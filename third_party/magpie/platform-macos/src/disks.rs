/* src/disks.rs
 *
 * Copyright 2025 Mission Center Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::collections::HashMap;
use std::process::Command;
use std::time::Instant;

use magpie_platform::disks::{
    Disk, DiskKind, DiskSmartData, DisksResponseErrorEjectFailed, PartitionInfo,
};
use magpie_platform::processes::Process;
use magpie_platform::Mutex;

#[derive(Clone, Default)]
struct RawDiskStats {
    bytes: Option<(u64, u64)>,
    timings: Option<(u64, u64)>,
}

struct DiskStats {
    raw: RawDiskStats,
    timestamp: Instant,
}

fn disk_rates(
    raw: &RawDiskStats,
    previous: Option<&DiskStats>,
    now: Instant,
) -> (Option<(u64, u64)>, Option<f32>) {
    let Some(prev) = previous else {
        return (None, None);
    };
    let elapsed = now.duration_since(prev.timestamp).as_secs_f64();
    if elapsed <= 0.0 {
        return (None, None);
    }
    let rates = raw
        .bytes
        .zip(prev.raw.bytes)
        .and_then(|((read, write), (old_read, old_write))| {
            Some((
                (read.checked_sub(old_read)? as f64 / elapsed) as u64,
                (write.checked_sub(old_write)? as f64 / elapsed) as u64,
            ))
        });
    let response =
        raw.timings
            .zip(prev.raw.timings)
            .and_then(|((time, ops), (old_time, old_ops))| {
                let delta_ops = ops.checked_sub(old_ops)?;
                if delta_ops == 0 {
                    return None;
                }
                Some((time.checked_sub(old_time)? as f64 / delta_ops as f64 / 1_000_000.0) as f32)
            });
    (rates, response)
}

pub struct DisksCache {
    disks: Vec<Disk>,
    prev: HashMap<String, DiskStats>,
}

impl magpie_platform::disks::DisksCache for DisksCache {
    fn new() -> Self {
        Self {
            disks: Vec::new(),
            prev: HashMap::new(),
        }
    }

    fn refresh(&mut self) {
        let now = Instant::now();
        let disk_names = list_physical_disks();
        let mounts = list_mount_points();
        let apfs = read_apfs_containers();
        let system_disk = detect_system_disk(&mounts, &apfs);
        let all_stats = read_all_disk_stats();

        let mut next_prev = HashMap::with_capacity(disk_names.len());
        let mut disks = Vec::with_capacity(disk_names.len());

        for bsd_name in disk_names {
            let raw = all_stats.get(&bsd_name).cloned().unwrap_or_default();
            let info = diskutil_info(&bsd_name);
            let (capacity, formatted) = capacity_for(&info);
            let kind = detect_disk_type(&info);
            let ejectable = info.ejectable;
            let is_system = system_disk.as_deref() == Some(bsd_name.as_str());
            let (rates, response) = disk_rates(&raw, self.prev.get(&bsd_name), now);
            let partitions = partitions_for(&bsd_name, &mounts, &apfs);
            let totals = raw.bytes;
            next_prev.insert(
                bsd_name.clone(),
                DiskStats {
                    raw,
                    timestamp: now,
                },
            );

            disks.push(Disk {
                id: bsd_name,
                model: info.model,
                kind: kind.map(|k| k as i32),
                smart_interface: None,
                capacity_bytes: capacity,
                formatted_bytes: formatted,
                is_system,
                busy_percent: f32::NAN,
                response_time_ms: response.unwrap_or(0.0),
                rx_speed_bytes_ps: rates.map_or(0, |r| r.0),
                rx_bytes_total: totals.map_or(0, |r| r.0),
                tx_speed_bytes_ps: rates.map_or(0, |r| r.1),
                tx_bytes_total: totals.map_or(0, |r| r.1),
                io_totals_available: Some(totals.is_some()),
                io_rates_available: Some(rates.is_some()),
                response_time_available: Some(response.is_some()),
                ejectable,
                temperature_milli_k: None,
                serial_number: info.serial,
                world_wide_name: None,
                rotation_rate: None,
                sector_size: info.sector_size.unwrap_or(512),
                partitions,
            });
        }

        self.prev = next_prev;
        self.disks = disks;
    }

    fn cached_entries(&self) -> &[Disk] {
        &self.disks
    }
}

pub struct DisksManager;

impl magpie_platform::disks::DisksManager for DisksManager {
    fn new() -> Self {
        Self
    }

    fn eject(
        &self,
        id: &str,
        _processes: &Mutex<HashMap<u32, Process>>,
    ) -> Result<(), DisksResponseErrorEjectFailed> {
        // Accept whole disks and partitions, but never pass arbitrary arguments to diskutil.
        let valid_id = id.strip_prefix("disk").is_some_and(|rest| {
            let (disk_number, partition_number) = rest.split_once('s').unwrap_or((rest, ""));
            !disk_number.is_empty()
                && disk_number.bytes().all(|byte| byte.is_ascii_digit())
                && (partition_number.is_empty()
                    || (!partition_number.is_empty()
                        && partition_number.bytes().all(|byte| byte.is_ascii_digit())))
        });
        if !valid_id {
            log::error!("Refusing to eject invalid macOS disk identifier: {id:?}");
            return Err(DisksResponseErrorEjectFailed {
                blockers: Vec::new(),
            });
        }

        match Command::new("/usr/sbin/diskutil")
            .args(["eject", id])
            .output()
        {
            Ok(output) if output.status.success() => Ok(()),
            Ok(output) => {
                log::error!(
                    "diskutil eject {id} failed with status {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                );
                Err(DisksResponseErrorEjectFailed {
                    blockers: Vec::new(),
                })
            }
            Err(error) => {
                log::error!("Failed to run diskutil eject {id}: {error}");
                Err(DisksResponseErrorEjectFailed {
                    blockers: Vec::new(),
                })
            }
        }
    }

    fn smart_data(&self, _id: &str) -> Option<DiskSmartData> {
        None
    }
}

#[derive(Default)]
struct DiskInfo {
    model: Option<String>,
    serial: Option<String>,
    ejectable: bool,
    total_size: Option<u64>,
    sector_size: Option<u64>,
    solid_state: Option<bool>,
    nvme: bool,
    optical: bool,
    sd_card: bool,
}

fn diskutil_plist(args: &[&str]) -> Option<plist::Value> {
    let output = Command::new("/usr/sbin/diskutil")
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    plist::Value::from_reader_xml(output.stdout.as_slice()).ok()
}

fn list_physical_disks() -> Vec<String> {
    diskutil_plist(&["list", "-plist", "physical"])
        .and_then(|v| v.as_dictionary()?.get("AllDisks")?.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_string())
        .filter(|name| {
            name.strip_prefix("disk")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .map(str::to_owned)
        .collect()
}

fn diskutil_info(bsd_name: &str) -> DiskInfo {
    let Some(value) = diskutil_plist(&["info", "-plist", bsd_name]) else {
        return DiskInfo::default();
    };
    let Some(dict) = value.as_dictionary() else {
        return DiskInfo::default();
    };
    let string = |key: &str| dict.get(key).and_then(|v| v.as_string()).map(str::to_owned);
    let model = string("MediaName").or_else(|| string("IORegistryEntryName"));
    let description = format!(
        "{} {}",
        model.as_deref().unwrap_or(""),
        string("BusProtocol").unwrap_or_default()
    )
    .to_lowercase();
    DiskInfo {
        model,
        serial: string("IORegistryEntrySerialNumber"),
        total_size: dict.get("TotalSize").and_then(|v| v.as_unsigned_integer()),
        sector_size: dict
            .get("DeviceBlockSize")
            .and_then(|v| v.as_unsigned_integer()),
        ejectable: dict
            .get("Ejectable")
            .and_then(|v| v.as_boolean())
            .unwrap_or(false),
        solid_state: dict.get("SolidState").and_then(|v| v.as_boolean()),
        nvme: description.contains("nvme"),
        optical: description.contains("optical")
            || description.contains("cd-rom")
            || description.contains("dvd"),
        sd_card: description.contains("sd card")
            || description.contains("sdxc")
            || description.contains("secure digital"),
    }
}

fn detect_disk_type(info: &DiskInfo) -> Option<DiskKind> {
    if info.optical {
        Some(DiskKind::Optical)
    } else if info.sd_card {
        Some(DiskKind::Sd)
    } else if info.nvme {
        Some(DiskKind::NvMe)
    } else {
        info.solid_state
            .map(|ssd| if ssd { DiskKind::Ssd } else { DiskKind::Hdd })
    }
}

fn capacity_for(info: &DiskInfo) -> (u64, Option<u64>) {
    // Formatted capacity is not the same as used space. diskutil does not
    // reliably report it for every partition layout.
    (info.total_size.unwrap_or(0), None)
}

fn read_all_disk_stats() -> HashMap<String, RawDiskStats> {
    let output = std::process::Command::new("/usr/sbin/ioreg")
        .args(["-r", "-d", "8", "-c", "IOBlockStorageDriver", "-l"])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            parse_all_ioreg_disk_stats(&text)
        }
        _ => HashMap::new(),
    }
}

fn parse_all_ioreg_disk_stats(text: &str) -> HashMap<String, RawDiskStats> {
    let mut map = HashMap::new();
    for block in text.split("+-o IOBlockStorageDriver") {
        let bsd = match extract_str_field(block, "BSD Name") {
            Some(b) => {
                let suffix = b.trim_start_matches("disk");
                if suffix.chars().all(|c| c.is_ascii_digit()) && !suffix.is_empty() {
                    b
                } else {
                    continue;
                }
            }
            _ => continue,
        };
        let bytes = extract_ioreg_stat(block, "Bytes (Read)")
            .zip(extract_ioreg_stat(block, "Bytes (Write)"));
        let timings = extract_ioreg_stat(block, "Total Time (Read)")
            .zip(extract_ioreg_stat(block, "Total Time (Write)"))
            .and_then(|(r, w)| r.checked_add(w))
            .zip(
                extract_ioreg_stat(block, "Operations (Read)")
                    .zip(extract_ioreg_stat(block, "Operations (Write)"))
                    .and_then(|(r, w)| r.checked_add(w)),
            );
        map.insert(bsd, RawDiskStats { bytes, timings });
    }
    map
}

fn extract_str_field(block: &str, key: &str) -> Option<String> {
    let search = format!("\"{key}\" = \"");
    let pos = block.find(&search)?;
    let rest = &block[pos + search.len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_ioreg_stat(block: &str, key: &str) -> Option<u64> {
    let search = format!("\"{key}\"=");
    let pos = block.find(&search)?;
    let rest = &block[pos + search.len()..];
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    num.parse().ok()
}

struct MountEntry {
    dev: String,
    mnt: String,
    total: u64,
    used: u64,
    filesystem: Option<String>,
}

fn list_mount_points() -> Vec<MountEntry> {
    unsafe {
        let mut fs_list: *mut libc::statfs = std::ptr::null_mut();
        let count = libc::getmntinfo(&mut fs_list, libc::MNT_NOWAIT);
        if count <= 0 {
            return vec![];
        }
        let slice = std::slice::from_raw_parts(fs_list, count as usize);
        slice
            .iter()
            .map(|fs| {
                let dev = std::ffi::CStr::from_ptr(fs.f_mntfromname.as_ptr())
                    .to_string_lossy()
                    .into_owned();
                let mnt = std::ffi::CStr::from_ptr(fs.f_mntonname.as_ptr())
                    .to_string_lossy()
                    .into_owned();
                let filesystem = std::ffi::CStr::from_ptr(fs.f_fstypename.as_ptr())
                    .to_string_lossy()
                    .into_owned();
                let bsize = fs.f_bsize as u64;
                let total = bsize.saturating_mul(fs.f_blocks as u64);
                let free = bsize.saturating_mul(fs.f_bfree as u64);
                let used = total.saturating_sub(free);
                MountEntry {
                    dev,
                    mnt,
                    total,
                    used,
                    filesystem: if filesystem.is_empty() {
                        None
                    } else {
                        Some(filesystem)
                    },
                }
            })
            .collect()
    }
}

fn strip_partition_suffix(bsd: &str) -> String {
    if let Some(rest) = bsd.strip_prefix("disk") {
        if let Some((number, _)) = rest.split_once('s') {
            return format!("disk{number}");
        }
    }
    bsd.to_string()
}

struct ApfsContainerInfo {
    container: String,
    physical_stores: Vec<String>,
    volumes: HashMap<String, u64>,
}

fn read_apfs_containers() -> Vec<ApfsContainerInfo> {
    let Some(value) = diskutil_plist(&["apfs", "list", "-plist"]) else {
        return vec![];
    };
    let Some(containers) = value
        .as_dictionary()
        .and_then(|d| d.get("Containers"))
        .and_then(|v| v.as_array())
    else {
        return vec![];
    };
    containers
        .iter()
        .filter_map(|v| {
            let d = v.as_dictionary()?;
            let container = d.get("ContainerReference")?.as_string()?.to_owned();
            let stores = d.get("PhysicalStores")?.as_array()?;
            let physical_stores = stores
                .iter()
                .filter_map(|v| v.as_dictionary()?.get("DeviceIdentifier")?.as_string())
                .map(strip_partition_suffix)
                .collect();
            let volumes = d
                .get("Volumes")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .filter_map(|v| {
                    let volume = v.as_dictionary()?;
                    Some((
                        volume.get("DeviceIdentifier")?.as_string()?.to_owned(),
                        volume.get("CapacityInUse")?.as_unsigned_integer()?,
                    ))
                })
                .collect();
            Some(ApfsContainerInfo {
                container,
                physical_stores,
                volumes,
            })
        })
        .collect()
}

fn belongs_to_disk(bsd: &str, disk: &str, apfs: &[ApfsContainerInfo]) -> bool {
    let whole = strip_partition_suffix(bsd);
    whole == disk
        || apfs
            .iter()
            .any(|c| c.container == whole && c.physical_stores.iter().any(|s| s == disk))
}

fn detect_system_disk(mounts: &[MountEntry], apfs: &[ApfsContainerInfo]) -> Option<String> {
    let root = mounts
        .iter()
        .find(|m| m.mnt == "/")?
        .dev
        .strip_prefix("/dev/")?;
    let whole = strip_partition_suffix(root);
    apfs.iter()
        .find(|c| c.container == whole)
        .and_then(|c| c.physical_stores.first().cloned())
        .or(Some(whole))
}

fn partitions_for(
    bsd_name: &str,
    mounts: &[MountEntry],
    apfs: &[ApfsContainerInfo],
) -> HashMap<String, PartitionInfo> {
    let mut map = HashMap::new();
    for m in mounts {
        let bsd = m.dev.trim_start_matches("/dev/");
        if !belongs_to_disk(bsd, bsd_name, apfs) {
            continue;
        }
        // Whole-disk mounts are rare; still include them.
        let key = bsd.to_string();
        let apfs_volume = apfs
            .iter()
            .find(|c| c.container == strip_partition_suffix(bsd));
        let volume = bsd
            .rsplit_once('s')
            .filter(|(parent, _)| {
                parent
                    .strip_prefix("disk")
                    .is_some_and(|rest| rest.contains('s'))
            })
            .map_or(bsd, |(parent, _)| parent);
        map.entry(key.clone()).or_insert_with(|| PartitionInfo {
            devname: m.dev.clone(),
            size: if apfs_volume.is_some() {
                None
            } else {
                Some(m.total)
            },
            used: apfs_volume
                .and_then(|c| c.volumes.get(volume).copied())
                .or_else(|| {
                    if apfs_volume.is_none() {
                        Some(m.used)
                    } else {
                        None
                    }
                }),
            filesystem: m.filesystem.clone(),
            mountpoints: vec![m.mnt.clone()],
        });
        if let Some(p) = map.get_mut(&key) {
            if !p.mountpoints.contains(&m.mnt) {
                p.mountpoints.push(m.mnt.clone());
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use magpie_platform::disks::DisksCache as _;

    #[test]
    fn distinguishes_missing_warmup_idle_and_reset() {
        let now = Instant::now();
        let raw = RawDiskStats {
            bytes: Some((100, 200)),
            timings: Some((1_000_000, 2)),
        };
        assert_eq!(disk_rates(&raw, None, now), (None, None));
        let prev = DiskStats {
            raw: raw.clone(),
            timestamp: now,
        };
        let later = now + std::time::Duration::from_secs(1);
        assert_eq!(disk_rates(&raw, Some(&prev), later), (Some((0, 0)), None));
        let active = RawDiskStats {
            bytes: Some((110, 220)),
            timings: Some((3_000_000, 4)),
        };
        assert_eq!(
            disk_rates(&active, Some(&prev), later),
            (Some((10, 20)), Some(1.0))
        );
        assert_eq!(
            disk_rates(&RawDiskStats::default(), Some(&prev), later),
            (None, None)
        );
        let reset = DiskStats {
            raw: active,
            timestamp: now,
        };
        assert_eq!(disk_rates(&raw, Some(&reset), later), (None, None));
        let parsed = parse_all_ioreg_disk_stats(
            "+-o IOBlockStorageDriver\n\"BSD Name\" = \"disk0\"\n\"Bytes (Read)\"=4",
        );
        assert!(parsed["disk0"].bytes.is_none());
    }

    #[test]
    fn maps_apfs_mounts_to_physical_stores() {
        let apfs = vec![ApfsContainerInfo {
            container: "disk3".into(),
            physical_stores: vec!["disk0".into()],
            volumes: HashMap::from([("disk3s1".into(), 40)]),
        }];
        let mounts = vec![MountEntry {
            dev: "/dev/disk3s1s1".into(),
            mnt: "/".into(),
            total: 100,
            used: 40,
            filesystem: Some("apfs".into()),
        }];
        assert_eq!(detect_system_disk(&mounts, &apfs).as_deref(), Some("disk0"));
        assert_eq!(
            partitions_for("disk0", &mounts, &apfs)["disk3s1s1"].used,
            Some(40)
        );
        assert!(partitions_for("disk1", &mounts, &apfs).is_empty());
    }

    #[test]
    #[ignore = "samples native disk counters"]
    fn native_disk_readings() {
        let mut cache = DisksCache::new();
        cache.refresh();
        for disk in cache.cached_entries() {
            println!("disk {disk:?}");
        }
        assert!(cache
            .cached_entries()
            .iter()
            .any(|d| d.is_system && d.capacity_bytes > 0 && !d.partitions.is_empty()));
        std::thread::sleep(std::time::Duration::from_secs(1));
        cache.refresh();
        for disk in cache.cached_entries() {
            println!("second disk {disk:?}");
        }
    }
}
