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
use std::time::Instant;

use magpie_platform::disks::{
    Disk, DiskKind, DiskSmartData, DisksResponseErrorEjectFailed, PartitionInfo,
};
use magpie_platform::processes::Process;
use magpie_platform::Mutex;

#[derive(Debug, Clone, Default)]
struct DiskStats {
    read_bytes: u64,
    write_bytes: u64,
    busy_time_ns: u64,
    total_ops: u64,
    timestamp: Option<Instant>,
}

#[derive(Default, Clone)]
struct RawDiskStats {
    read_bytes: u64,
    write_bytes: u64,
    busy_time_ns: u64,
    read_ops: u64,
    write_ops: u64,
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
        let system_disk = detect_system_disk(&mounts);
        let all_stats = read_all_disk_stats();
        let apfs = read_apfs_containers();

        let mut next_prev = HashMap::with_capacity(disk_names.len());
        let mut disks = Vec::with_capacity(disk_names.len());

        for bsd_name in disk_names {
            let raw = all_stats.get(&bsd_name).cloned().unwrap_or_default();
            let info = diskutil_info(&bsd_name);
            let (capacity, formatted) = capacity_for(&bsd_name, &info, &apfs);
            let kind = detect_disk_type(&info);
            let ejectable = info.ejectable;
            let is_system = system_disk.as_deref() == Some(bsd_name.as_str());

            let prev_stats = self.prev.get(&bsd_name).cloned().unwrap_or_default();
            let prev_ts = prev_stats.timestamp.unwrap_or(now);
            let elapsed_secs = now.duration_since(prev_ts).as_secs_f64().max(0.001);

            let read_delta = raw.read_bytes.saturating_sub(prev_stats.read_bytes);
            let write_delta = raw.write_bytes.saturating_sub(prev_stats.write_bytes);
            let busy_delta_ns = raw.busy_time_ns.saturating_sub(prev_stats.busy_time_ns);
            let ops_delta = raw
                .read_ops
                .saturating_add(raw.write_ops)
                .saturating_sub(prev_stats.total_ops);

            let read_speed = (read_delta as f64 / elapsed_secs) as u64;
            let write_speed = (write_delta as f64 / elapsed_secs) as u64;
            let busy_percent =
                ((busy_delta_ns as f64 / (elapsed_secs * 1_000_000_000.0)) * 100.0).min(100.0)
                    as f32;
            let response_time_ms = if ops_delta > 0 {
                (busy_delta_ns as f64 / ops_delta as f64 / 1_000_000.0) as f32
            } else {
                0.0
            };

            let partitions = partitions_for(&bsd_name, &mounts);

            next_prev.insert(
                bsd_name.clone(),
                DiskStats {
                    read_bytes: raw.read_bytes,
                    write_bytes: raw.write_bytes,
                    busy_time_ns: raw.busy_time_ns,
                    total_ops: raw.read_ops.saturating_add(raw.write_ops),
                    timestamp: Some(now),
                },
            );

            disks.push(Disk {
                id: bsd_name,
                model: info.model,
                kind: kind.map(|k| k as i32),
                smart_interface: None,
                capacity_bytes: capacity,
                formatted_bytes: Some(formatted),
                is_system,
                busy_percent,
                response_time_ms,
                rx_speed_bytes_ps: read_speed,
                rx_bytes_total: raw.read_bytes,
                tx_speed_bytes_ps: write_speed,
                tx_bytes_total: raw.write_bytes,
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
        _id: &str,
        _processes: &Mutex<HashMap<u32, Process>>,
    ) -> Result<(), DisksResponseErrorEjectFailed> {
        // ponytail: eject unsupported on macOS port for now; wire diskutil eject when needed.
        Err(DisksResponseErrorEjectFailed {
            blockers: Vec::new(),
        })
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
    solid_state: bool,
    nvme: bool,
    optical: bool,
    sd_card: bool,
    virtual_disk: bool,
}

fn list_physical_disks() -> Vec<String> {
    let output = std::process::Command::new("/usr/sbin/diskutil")
        .args(["list", "-plist"])
        .output();
    let all_disks = match output {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            parse_diskutil_list(&text)
        }
        _ => return vec![],
    };

    all_disks
        .into_iter()
        .filter(|name| {
            let info = diskutil_info(name);
            !info.virtual_disk
        })
        .collect()
}

fn parse_diskutil_list(plist: &str) -> Vec<String> {
    let mut disks = vec![];
    let mut in_all_disks = false;
    for line in plist.lines() {
        let trimmed = line.trim();
        if trimmed.contains("<key>AllDisks</key>") {
            in_all_disks = true;
            continue;
        }
        if in_all_disks {
            if trimmed.starts_with("</array>") {
                break;
            }
            if let Some(start) = trimmed.find("<string>disk") {
                let rest = &trimmed[start + 8..];
                if let Some(end) = rest.find("</string>") {
                    let name = &rest[..end];
                    let suffix = &name[4..];
                    if suffix.chars().all(|c| c.is_ascii_digit()) {
                        disks.push(name.to_string());
                    }
                }
            }
        }
    }
    disks
}

fn diskutil_info(bsd_name: &str) -> DiskInfo {
    let output = std::process::Command::new("/usr/sbin/diskutil")
        .args(["info", "-plist", bsd_name])
        .output();
    let text = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => return DiskInfo::default(),
    };

    let mut info = DiskInfo::default();
    info.model = parse_plist_str(&text, "MediaName").or_else(|| parse_plist_str(&text, "IORegistryEntryName"));
    info.serial = parse_plist_str(&text, "IORegistryEntrySerialNumber")
        .or_else(|| parse_plist_str(&text, "DeviceIdentifier"));
    info.total_size = parse_plist_u64(&text, "TotalSize");
    info.sector_size = parse_plist_u64(&text, "DeviceBlockSize");
    info.ejectable = parse_plist_bool(&text, "Ejectable").unwrap_or(false);
    info.virtual_disk = parse_plist_str(&text, "VirtualOrPhysical")
        .map(|v| v.contains("Virtual"))
        .unwrap_or(false);

    let lower = text.to_lowercase();
    info.solid_state = lower.contains("solid state")
        || lower.contains("<string>ssd</string>")
        || lower.contains("flash");
    info.nvme = lower.contains("nvme");
    info.optical = lower.contains("optical") || lower.contains("cd-rom") || lower.contains("dvd");
    info.sd_card = lower.contains("sd card") || lower.contains("sdxc") || lower.contains("secure digital");

    // Prefer human-readable model from plain `diskutil info` when plist MediaName is weak.
    if info.model.is_none() {
        if let Ok(o) = std::process::Command::new("/usr/sbin/diskutil")
            .args(["info", bsd_name])
            .output()
        {
            let plain = String::from_utf8_lossy(&o.stdout);
            for line in plain.lines() {
                if line.contains("Device / Media Name:") {
                    if let Some(val) = line.splitn(2, ':').nth(1) {
                        let v = val.trim();
                        if !v.is_empty() {
                            info.model = Some(v.to_string());
                        }
                    }
                }
            }
        }
    }

    info
}

fn detect_disk_type(info: &DiskInfo) -> Option<DiskKind> {
    if info.optical {
        Some(DiskKind::Optical)
    } else if info.sd_card {
        Some(DiskKind::Sd)
    } else if info.nvme {
        Some(DiskKind::NvMe)
    } else if info.solid_state {
        Some(DiskKind::Ssd)
    } else {
        Some(DiskKind::Hdd)
    }
}

fn capacity_for(bsd_name: &str, info: &DiskInfo, apfs: &[ApfsContainerInfo]) -> (u64, u64) {
    let total = info.total_size.unwrap_or(0);
    let free: u64 = apfs
        .iter()
        .filter(|c| c.physical_store == bsd_name)
        .map(|c| c.free)
        .sum();
    let formatted = if free > 0 {
        total.saturating_sub(free)
    } else {
        total
    };
    (total, formatted)
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
        let read_bytes = extract_ioreg_stat(block, "Bytes (Read)").unwrap_or(0);
        let write_bytes = extract_ioreg_stat(block, "Bytes (Write)").unwrap_or(0);
        let total_time_read = extract_ioreg_stat(block, "Total Time (Read)").unwrap_or(0);
        let total_time_write = extract_ioreg_stat(block, "Total Time (Write)").unwrap_or(0);
        let busy_time_ns = total_time_read + total_time_write;
        let read_ops = extract_ioreg_stat(block, "Operations (Read)").unwrap_or(0);
        let write_ops = extract_ioreg_stat(block, "Operations (Write)").unwrap_or(0);
        map.insert(
            bsd,
            RawDiskStats {
                read_bytes,
                write_bytes,
                busy_time_ns,
                read_ops,
                write_ops,
            },
        );
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

fn detect_system_disk(mounts: &[MountEntry]) -> Option<String> {
    for m in mounts {
        if m.mnt == "/" {
            let bsd = m.dev.trim_start_matches("/dev/");
            return physical_disk_for_bsd(bsd);
        }
    }
    None
}

fn strip_partition_suffix(bsd: &str) -> String {
    if let Some(pos) = bsd.rfind('s') {
        let suffix = &bsd[pos + 1..];
        if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
            return bsd[..pos].to_string();
        }
    }
    bsd.to_string()
}

fn physical_disk_for_bsd(bsd: &str) -> Option<String> {
    let output = std::process::Command::new("/usr/sbin/diskutil")
        .args(["info", "-plist", bsd])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);

    let parent = parse_plist_str(&text, "ParentWholeDisk")?;
    if parent == bsd {
        if let Some(store) = parse_plist_str(&text, "APFSPhysicalStore") {
            return Some(strip_partition_suffix(&store));
        }
        return Some(parent);
    }
    physical_disk_for_bsd(&parent)
}

struct ApfsContainerInfo {
    free: u64,
    physical_store: String,
}

fn read_apfs_containers() -> Vec<ApfsContainerInfo> {
    let output = std::process::Command::new("/usr/sbin/diskutil")
        .args(["apfs", "list", "-plist"])
        .output();
    let text = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => return vec![],
    };

    let mut result = vec![];
    let mut current_free: u64 = 0;
    let mut current_ceiling: u64 = 0;
    let mut current_store = String::new();
    let mut next_key = String::new();

    for line in text.lines() {
        let t = line.trim();

        if t.starts_with("<key>") && t.ends_with("</key>") {
            next_key = t[5..t.len() - 6].to_string();
            continue;
        }

        if let Some(v) = t
            .strip_prefix("<integer>")
            .and_then(|s| s.strip_suffix("</integer>"))
        {
            if let Ok(n) = v.parse::<u64>() {
                match next_key.as_str() {
                    "CapacityFree" => current_free = n,
                    "CapacityCeiling" => current_ceiling = n,
                    _ => {}
                }
            }
            continue;
        }

        if let Some(v) = t
            .strip_prefix("<string>")
            .and_then(|s| s.strip_suffix("</string>"))
        {
            if next_key == "DesignatedPhysicalStore" {
                current_store = strip_partition_suffix(v);
            }
            continue;
        }

        if t == "</dict>" && !current_store.is_empty() && current_ceiling > 0 {
            result.push(ApfsContainerInfo {
                free: current_free,
                physical_store: current_store.clone(),
            });
            current_free = 0;
            current_ceiling = 0;
            current_store.clear();
        }
    }
    result
}

fn partitions_for(bsd_name: &str, mounts: &[MountEntry]) -> HashMap<String, PartitionInfo> {
    let mut map = HashMap::new();
    for m in mounts {
        let bsd = m.dev.trim_start_matches("/dev/");
        if strip_partition_suffix(bsd) != bsd_name {
            continue;
        }
        // Whole-disk mounts are rare; still include them.
        let key = bsd.to_string();
        map.entry(key.clone()).or_insert_with(|| PartitionInfo {
            devname: m.dev.clone(),
            size: Some(m.total),
            used: Some(m.used),
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

fn parse_plist_str(text: &str, key: &str) -> Option<String> {
    let search = format!("<key>{key}</key>");
    let mut next = false;
    for line in text.lines() {
        let t = line.trim();
        if t == search {
            next = true;
            continue;
        }
        if next {
            if let Some(v) = t
                .strip_prefix("<string>")
                .and_then(|s| s.strip_suffix("</string>"))
            {
                return Some(v.to_string());
            }
            return None;
        }
    }
    None
}

fn parse_plist_u64(text: &str, key: &str) -> Option<u64> {
    let search = format!("<key>{key}</key>");
    let mut next = false;
    for line in text.lines() {
        let t = line.trim();
        if t == search {
            next = true;
            continue;
        }
        if next {
            if let Some(v) = t
                .strip_prefix("<integer>")
                .and_then(|s| s.strip_suffix("</integer>"))
            {
                return v.parse().ok();
            }
            return None;
        }
    }
    None
}

fn parse_plist_bool(text: &str, key: &str) -> Option<bool> {
    let search = format!("<key>{key}</key>");
    let mut next = false;
    for line in text.lines() {
        let t = line.trim();
        if t == search {
            next = true;
            continue;
        }
        if next {
            return match t {
                "<true/>" | "<true />" => Some(true),
                "<false/>" | "<false />" => Some(false),
                _ => None,
            };
        }
    }
    None
}
