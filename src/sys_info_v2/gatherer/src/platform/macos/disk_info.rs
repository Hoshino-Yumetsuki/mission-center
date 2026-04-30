#![allow(dead_code)]
use crate::dbus_shim::{Append, Arg, ArgType, IterAppend, Signature};
use std::sync::Arc;
use std::time::Instant;

use crate::platform::disk_info::{DiskInfoExt, DiskType, DisksInfoExt};

use super::{INITIAL_REFRESH_TS, MIN_DELTA_REFRESH};

#[derive(Debug, Clone, PartialEq)]
pub struct MacosDiskInfo {
    pub id: Arc<str>,
    pub model: Arc<str>,
    pub r#type: DiskType,
    pub capacity: u64,
    pub formatted: u64,
    pub system_disk: bool,
    pub busy_percent: f32,
    pub response_time_ms: f32,
    pub read_speed: u64,
    pub write_speed: u64,
}

impl Default for MacosDiskInfo {
    fn default() -> Self {
        Self {
            id: Arc::from(""),
            model: Arc::from(""),
            r#type: DiskType::default(),
            capacity: 0,
            formatted: 0,
            system_disk: false,
            busy_percent: 0.0,
            response_time_ms: 0.0,
            read_speed: 0,
            write_speed: 0,
        }
    }
}

impl DiskInfoExt for MacosDiskInfo {
    fn id(&self) -> &str { self.id.as_ref() }
    fn model(&self) -> &str { self.model.as_ref() }
    fn r#type(&self) -> DiskType { self.r#type }
    fn capacity(&self) -> u64 { self.capacity }
    fn formatted(&self) -> u64 { self.formatted }
    fn is_system_disk(&self) -> bool { self.system_disk }
    fn busy_percent(&self) -> f32 { self.busy_percent }
    fn response_time_ms(&self) -> f32 { self.response_time_ms }
    fn read_speed(&self) -> u64 { self.read_speed }
    fn write_speed(&self) -> u64 { self.write_speed }
}

#[derive(Debug, Clone, Default)]
pub(super) struct DiskStats {
    read_bytes: u64,
    write_bytes: u64,
    busy_time_ns: u64,
    total_ops: u64,
    timestamp: Option<Instant>,
}

pub struct MacosDiskInfoIter<'a>(
    std::iter::Map<
        std::slice::Iter<'a, (DiskStats, MacosDiskInfo)>,
        fn(&'a (DiskStats, MacosDiskInfo)) -> &'a MacosDiskInfo,
    >,
);

impl<'a> Iterator for MacosDiskInfoIter<'a> {
    type Item = &'a MacosDiskInfo;
    fn next(&mut self) -> Option<Self::Item> { self.0.next() }
}

impl<'a> Clone for MacosDiskInfoIter<'a> {
    fn clone(&self) -> Self { MacosDiskInfoIter(self.0.clone()) }
}

pub struct MacosDisksInfo {
    disks: Vec<(DiskStats, MacosDiskInfo)>,
    last_refresh: Instant,
}

impl Default for MacosDisksInfo {
    fn default() -> Self {
        Self {
            disks: vec![],
            last_refresh: *INITIAL_REFRESH_TS,
        }
    }
}

impl MacosDisksInfo {
    pub fn new() -> Self { Self::default() }
}

impl<'a> DisksInfoExt<'a> for MacosDisksInfo {
    type S = MacosDiskInfo;
    type Iter = MacosDiskInfoIter<'a>;

    fn refresh_cache(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_refresh) < MIN_DELTA_REFRESH {
            return;
        }
        self.last_refresh = now;
        self.disks = enumerate_disks(now, &self.disks);
    }

    fn info(&'a self) -> Self::Iter {
        MacosDiskInfoIter(self.disks.iter().map(|(_, d)| d as &MacosDiskInfo))
    }
}

fn enumerate_disks(
    now: Instant,
    prev: &[(DiskStats, MacosDiskInfo)],
) -> Vec<(DiskStats, MacosDiskInfo)> {
    let mut result = vec![];
    let disk_names = list_iokit_disks();
    let mounts = list_mount_points();
    let system_disk = detect_system_disk(&mounts);
    let all_stats = read_all_disk_stats();

    for bsd_name in disk_names {
        let raw = all_stats.get(&bsd_name).cloned().unwrap_or_default();
        let model = read_iokit_disk_model(&bsd_name);
        let disk_type = detect_disk_type(&bsd_name);
        let (capacity, formatted) = read_disk_capacity(&bsd_name, &mounts);

        let prev_entry = prev.iter().find(|(_, d)| d.id.as_ref() == bsd_name.as_str());
        let prev_stats = prev_entry.map(|(s, _)| s.clone()).unwrap_or_default();
        let prev_ts = prev_stats.timestamp.unwrap_or(now);
        let elapsed_secs = now.duration_since(prev_ts).as_secs_f64().max(0.001);

        let read_delta = raw.read_bytes.saturating_sub(prev_stats.read_bytes);
        let write_delta = raw.write_bytes.saturating_sub(prev_stats.write_bytes);
        let busy_delta_ns = raw.busy_time_ns.saturating_sub(prev_stats.busy_time_ns);
        let ops_delta = raw.read_ops.saturating_add(raw.write_ops)
            .saturating_sub(prev_stats.total_ops);

        let read_speed = (read_delta as f64 / elapsed_secs) as u64;
        let write_speed = (write_delta as f64 / elapsed_secs) as u64;
        let busy_percent =
            ((busy_delta_ns as f64 / (elapsed_secs * 1_000_000_000.0)) * 100.0).min(100.0) as f32;
        let response_time_ms = if ops_delta > 0 {
            (busy_delta_ns as f64 / ops_delta as f64 / 1_000_000.0) as f32
        } else {
            0.0
        };

        let is_system = system_disk.as_deref() == Some(bsd_name.as_str());

        result.push((
            DiskStats {
                read_bytes: raw.read_bytes,
                write_bytes: raw.write_bytes,
                busy_time_ns: raw.busy_time_ns,
                total_ops: raw.read_ops.saturating_add(raw.write_ops),
                timestamp: Some(now),
            },
            MacosDiskInfo {
                id: Arc::from(bsd_name.as_str()),
                model: Arc::from(model.as_str()),
                r#type: disk_type,
                capacity,
                formatted,
                system_disk: is_system,
                busy_percent,
                response_time_ms,
                read_speed,
                write_speed,
            },
        ));
    }
    result
}

fn list_iokit_disks() -> Vec<String> {
    let output = std::process::Command::new("diskutil")
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
        .filter(|name| !is_virtual_disk(name))
        .collect()
}

fn is_virtual_disk(disk_name: &str) -> bool {
    let output = std::process::Command::new("diskutil")
        .args(["info", "-plist", disk_name])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            let mut next_is_value = false;
            for line in text.lines() {
                let t = line.trim();
                if t == "<key>VirtualOrPhysical</key>" {
                    next_is_value = true;
                    continue;
                }
                if next_is_value {
                    return t.contains("Virtual");
                }
            }
            false
        }
        _ => false,
    }
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

#[derive(Default, Clone)]
struct RawDiskStats {
    read_bytes: u64,
    write_bytes: u64,
    busy_time_ns: u64,
    read_ops: u64,
    write_ops: u64,
}

fn read_all_disk_stats() -> std::collections::HashMap<String, RawDiskStats> {
    let output = std::process::Command::new("/usr/sbin/ioreg")
        .args(["-r", "-d", "8", "-c", "IOBlockStorageDriver", "-l"])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            parse_all_ioreg_disk_stats(&text)
        }
        _ => std::collections::HashMap::new(),
    }
}

fn parse_all_ioreg_disk_stats(text: &str) -> std::collections::HashMap<String, RawDiskStats> {
    let mut map = std::collections::HashMap::new();
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
        map.insert(bsd, RawDiskStats { read_bytes, write_bytes, busy_time_ns, read_ops, write_ops });
    }
    map
}

fn extract_str_field(block: &str, key: &str) -> Option<String> {
    let search = format!("\"{}\" = \"", key);
    let pos = block.find(&search)?;
    let rest = &block[pos + search.len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_ioreg_stat(block: &str, key: &str) -> Option<u64> {
    let search = format!("\"{}\"=", key);
    let pos = block.find(&search)?;
    let rest = &block[pos + search.len()..];
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    num.parse().ok()
}

fn read_iokit_disk_model(bsd_name: &str) -> String {
    let output = std::process::Command::new("diskutil")
        .args(["info", bsd_name])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            for line in text.lines() {
                if line.contains("Device / Media Name:") {
                    if let Some(val) = line.splitn(2, ':').nth(1) {
                        return val.trim().to_string();
                    }
                }
            }
            bsd_name.to_string()
        }
        _ => bsd_name.to_string(),
    }
}

fn detect_disk_type(bsd_name: &str) -> DiskType {
    let output = std::process::Command::new("diskutil")
        .args(["info", bsd_name])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout).to_lowercase();
            if text.contains("solid state") || text.contains("ssd") || text.contains("flash") {
                if text.contains("nvme") { DiskType::NVMe } else { DiskType::SSD }
            } else if text.contains("optical") || text.contains("cd") || text.contains("dvd") {
                DiskType::Optical
            } else if text.contains("sd card") || text.contains("sdxc") {
                DiskType::SD
            } else {
                DiskType::HDD
            }
        }
        _ => DiskType::Unknown,
    }
}

fn list_mount_points() -> Vec<(String, String)> {
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
                (dev, mnt)
            })
            .collect()
    }
}

fn detect_system_disk(mounts: &[(String, String)]) -> Option<String> {
    for (dev, mnt) in mounts {
        if mnt == "/" {
            let bsd = dev.trim_start_matches("/dev/");
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
    if !output.status.success() { return None; }
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
    ceiling: u64,
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
            next_key = t[5..t.len()-6].to_string();
            continue;
        }

        if let Some(v) = t.strip_prefix("<integer>").and_then(|s| s.strip_suffix("</integer>")) {
            if let Ok(n) = v.parse::<u64>() {
                match next_key.as_str() {
                    "CapacityFree" => current_free = n,
                    "CapacityCeiling" => current_ceiling = n,
                    _ => {}
                }
            }
            continue;
        }

        if let Some(v) = t.strip_prefix("<string>").and_then(|s| s.strip_suffix("</string>")) {
            if next_key == "DesignatedPhysicalStore" {
                current_store = strip_partition_suffix(v);
            }
            continue;
        }

        if t == "</dict>" && !current_store.is_empty() && current_ceiling > 0 {
            result.push(ApfsContainerInfo {
                free: current_free,
                ceiling: current_ceiling,
                physical_store: current_store.clone(),
            });
            current_free = 0;
            current_ceiling = 0;
            current_store.clear();
        }
    }
    result
}

fn read_disk_capacity(bsd_name: &str, _mounts: &[(String, String)]) -> (u64, u64) {
    let total = {
        let output = std::process::Command::new("/usr/sbin/diskutil")
            .args(["info", "-plist", bsd_name])
            .output();
        match output {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout);
                parse_plist_u64(&text, "TotalSize").unwrap_or(0)
            }
            _ => 0,
        }
    };

    let containers = read_apfs_containers();
    let free: u64 = containers.iter()
        .filter(|c| c.physical_store == bsd_name)
        .map(|c| c.free)
        .sum();

    let formatted = total.saturating_sub(free);
    (total, formatted)
}

fn parse_plist_str(text: &str, key: &str) -> Option<String> {
    let search = format!("<key>{}</key>", key);
    let mut next = false;
    for line in text.lines() {
        let t = line.trim();
        if t == search { next = true; continue; }
        if next {
            if let Some(v) = t.strip_prefix("<string>").and_then(|s| s.strip_suffix("</string>")) {
                return Some(v.to_string());
            }
            return None;
        }
    }
    None
}

fn parse_plist_u64(text: &str, key: &str) -> Option<u64> {
    let search = format!("<key>{}</key>", key);
    let mut next = false;
    for line in text.lines() {
        let t = line.trim();
        if t == search { next = true; continue; }
        if next {
            if let Some(v) = t.strip_prefix("<integer>").and_then(|s| s.strip_suffix("</integer>")) {
                return v.parse().ok();
            }
            return None;
        }
    }
    None
}

impl Arg for MacosDiskInfo {
    const ARG_TYPE: ArgType = ArgType::Struct;
    fn signature() -> Signature { Signature::from("") }
}
impl Append for MacosDiskInfo {
    fn append_by_ref(&self, _: &mut IterAppend) {}
}
