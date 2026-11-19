/* src/gpus.rs
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

use magpie_platform::gpus::{ApiVersion, Gpu};

pub struct GpuCache {
    gpus: HashMap<String, Gpu>,
    static_loaded: bool,
    #[cfg(target_arch = "aarch64")]
    apple_report: Option<AppleGpuReport>,
}

impl magpie_platform::gpus::GpuCache for GpuCache {
    fn new() -> Self {
        Self {
            gpus: HashMap::new(),
            static_loaded: false,
            #[cfg(target_arch = "aarch64")]
            apple_report: None,
        }
    }

    fn refresh(&mut self) {
        if !self.static_loaded {
            self.gpus = enumerate_static_gpus();
            self.static_loaded = !self.gpus.is_empty();
            #[cfg(target_arch = "aarch64")]
            {
                self.apple_report = AppleGpuReport::new();
            }
        }
        refresh_dynamic(
            &mut self.gpus,
            #[cfg(target_arch = "aarch64")]
            self.apple_report.as_mut(),
        );
    }

    fn cached_entries(&self) -> &HashMap<String, Gpu> {
        &self.gpus
    }
}

fn enumerate_static_gpus() -> HashMap<String, Gpu> {
    let text = command_text(&["/usr/sbin/system_profiler", "SPDisplaysDataType"]);

    let mut gpus = HashMap::new();
    let mut idx = 0u32;
    let mut name: Option<String> = None;
    let mut vendor_id = 0u32;
    let mut device_id = 0u32;
    let mut metal: Option<ApiVersion> = None;
    let mut total_memory = None;

    let flush = |gpus: &mut HashMap<String, Gpu>,
                 idx: &mut u32,
                 name: &mut Option<String>,
                 vendor_id: &mut u32,
                 device_id: &mut u32,
                 metal: &mut Option<ApiVersion>,
                 total_memory: &mut Option<u64>| {
        let Some(device_name) = name.take() else {
            return;
        };
        let id = format!("gpu-{idx}");
        *idx += 1;
        gpus.insert(
            id.clone(),
            Gpu {
                id,
                device_name: Some(device_name),
                vendor_id: *vendor_id,
                device_id: *device_id,
                total_memory: *total_memory,
                metal_version: metal.take(),
                encode_decode_shared: true,
                ..Default::default()
            },
        );
        *vendor_id = 0;
        *device_id = 0;
        *total_memory = None;
    };

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "Graphics/Displays:" {
            continue;
        }

        let indent = line.len() - line.trim_start().len();
        // GPU sections are indent-4 headings that are not property lines.
        if indent == 4 && trimmed.ends_with(':') && !is_display_property(trimmed) {
            flush(
                &mut gpus,
                &mut idx,
                &mut name,
                &mut vendor_id,
                &mut device_id,
                &mut metal,
                &mut total_memory,
            );
            name = Some(trimmed.trim_end_matches(':').to_string());
            continue;
        }

        if let Some(v) = field(trimmed, "Chipset Model") {
            name = Some(v);
        } else if let Some(v) = field(trimmed, "Vendor") {
            vendor_id = parse_vendor_id(&v);
        } else if let Some(v) = field(trimmed, "Device ID") {
            device_id = parse_hex_u32(&v).unwrap_or(0);
        } else if let Some(v) = field(trimmed, "Metal Support") {
            metal = parse_metal_version(&v);
        } else if let Some(v) = field(trimmed, "VRAM (Total)").or_else(|| field(trimmed, "VRAM")) {
            total_memory = parse_vram(&v);
        }
    }

    flush(
        &mut gpus,
        &mut idx,
        &mut name,
        &mut vendor_id,
        &mut device_id,
        &mut metal,
        &mut total_memory,
    );

    gpus
}

fn refresh_dynamic(
    gpus: &mut HashMap<String, Gpu>,
    #[cfg(target_arch = "aarch64")] report: Option<&mut AppleGpuReport>,
) {
    // Values are optional samples: clear them before every refresh so a real zero
    // and an unavailable sensor cannot leave a previous value looking current.
    for gpu in gpus.values_mut() {
        gpu.utilization_percent = None;
        gpu.power_draw_watts = None;
        gpu.clock_speed_mhz = None;
        gpu.used_memory = None;
        gpu.used_shared_memory = None;
        gpu.total_shared_memory = None;
    }

    // IOAccelerator output is not correlated with system_profiler GPU IDs.
    // Only associate it when there is exactly one GPU.
    if gpus.len() != 1 {
        return;
    }
    let gpu = gpus.values_mut().next().unwrap();
    let stats = read_ioaccelerator_stats();
    gpu.utilization_percent = stats.util_percent;
    if gpu.vendor_id == 0x106b {
        gpu.used_shared_memory = stats.used_memory;
    }

    #[cfg(target_arch = "aarch64")]
    if gpu.vendor_id == 0x106b {
        if let Some(stats) = report.and_then(|report| report.sample()) {
            gpu.utilization_percent = stats.utilization.or(gpu.utilization_percent);
            gpu.power_draw_watts = stats.power_watts;
            gpu.clock_speed_mhz = stats.frequency_mhz;
        }
    }
}

#[cfg(target_arch = "aarch64")]
unsafe impl Send for AppleGpuReport {}

#[cfg(target_arch = "aarch64")]
struct AppleGpuReport {
    handle: *mut libc::c_void,
    sub: *mut libc::c_void,
    channels: *mut libc::c_void,
    previous: *mut libc::c_void,
    previous_time: Option<std::time::Instant>,
    frequencies: Vec<u32>,
    sample: unsafe extern "C" fn(
        *mut libc::c_void,
        *mut libc::c_void,
        *mut libc::c_void,
    ) -> *mut libc::c_void,
    delta: unsafe extern "C" fn(
        *mut libc::c_void,
        *mut libc::c_void,
        *mut libc::c_void,
    ) -> *mut libc::c_void,
    group: unsafe extern "C" fn(*mut libc::c_void) -> *mut libc::c_void,
    subgroup: unsafe extern "C" fn(*mut libc::c_void) -> *mut libc::c_void,
    channel: unsafe extern "C" fn(*mut libc::c_void) -> *mut libc::c_void,
    unit: unsafe extern "C" fn(*mut libc::c_void) -> *mut libc::c_void,
    value: unsafe extern "C" fn(*mut libc::c_void, i32) -> i64,
    count: unsafe extern "C" fn(*mut libc::c_void) -> i32,
    state: unsafe extern "C" fn(*mut libc::c_void, i32) -> *mut libc::c_void,
    residency: unsafe extern "C" fn(*mut libc::c_void, i32) -> i64,
}

#[cfg(target_arch = "aarch64")]
struct AppleGpuStats {
    utilization: Option<f32>,
    frequency_mhz: Option<u32>,
    power_watts: Option<f32>,
}

#[cfg(target_arch = "aarch64")]
impl AppleGpuReport {
    fn new() -> Option<Self> {
        unsafe {
            let path = std::ffi::CString::new("/usr/lib/libIOReport.dylib").ok()?;
            let handle = libc::dlopen(path.as_ptr(), libc::RTLD_LAZY);
            if handle.is_null() {
                return None;
            }
            macro_rules! sym {
                ($n:literal, $t:ty) => {{
                    let n = std::ffi::CString::new($n).ok()?;
                    let p = libc::dlsym(handle, n.as_ptr());
                    if p.is_null() {
                        libc::dlclose(handle);
                        return None;
                    }
                    std::mem::transmute::<*mut libc::c_void, $t>(p)
                }};
            }
            let copy = sym!(
                "IOReportCopyChannelsInGroup",
                unsafe extern "C" fn(
                    *mut libc::c_void,
                    *mut libc::c_void,
                    u64,
                    u64,
                    u64,
                ) -> *mut libc::c_void
            );
            let merge = sym!(
                "IOReportMergeChannels",
                unsafe extern "C" fn(*mut libc::c_void, *mut libc::c_void, *mut libc::c_void)
            );
            let create_sub = sym!(
                "IOReportCreateSubscription",
                unsafe extern "C" fn(
                    *mut libc::c_void,
                    *mut libc::c_void,
                    *mut *mut libc::c_void,
                    u64,
                    *mut libc::c_void,
                ) -> *mut libc::c_void
            );
            let sample = sym!(
                "IOReportCreateSamples",
                unsafe extern "C" fn(
                    *mut libc::c_void,
                    *mut libc::c_void,
                    *mut libc::c_void,
                ) -> *mut libc::c_void
            );
            let delta = sym!(
                "IOReportCreateSamplesDelta",
                unsafe extern "C" fn(
                    *mut libc::c_void,
                    *mut libc::c_void,
                    *mut libc::c_void,
                ) -> *mut libc::c_void
            );
            let group = sym!(
                "IOReportChannelGetGroup",
                unsafe extern "C" fn(*mut libc::c_void) -> *mut libc::c_void
            );
            let subgroup = sym!(
                "IOReportChannelGetSubGroup",
                unsafe extern "C" fn(*mut libc::c_void) -> *mut libc::c_void
            );
            let channel = sym!(
                "IOReportChannelGetChannelName",
                unsafe extern "C" fn(*mut libc::c_void) -> *mut libc::c_void
            );
            let value = sym!(
                "IOReportSimpleGetIntegerValue",
                unsafe extern "C" fn(*mut libc::c_void, i32) -> i64
            );
            let unit = sym!(
                "IOReportChannelGetUnitLabel",
                unsafe extern "C" fn(*mut libc::c_void) -> *mut libc::c_void
            );
            let count = sym!(
                "IOReportStateGetCount",
                unsafe extern "C" fn(*mut libc::c_void) -> i32
            );
            let state = sym!(
                "IOReportStateGetNameForIndex",
                unsafe extern "C" fn(*mut libc::c_void, i32) -> *mut libc::c_void
            );
            let residency = sym!(
                "IOReportStateGetResidency",
                unsafe extern "C" fn(*mut libc::c_void, i32) -> i64
            );
            let cf_group = make_cf_string("GPU Stats")?;
            let cf_subgroup = make_cf_string("GPU Performance States")?;
            let channels = copy(cf_group, cf_subgroup, 0, 0, 0);
            CFRelease(cf_group);
            CFRelease(cf_subgroup);
            if channels.is_null() {
                libc::dlclose(handle);
                return None;
            }
            let energy_name = make_cf_string("Energy Model")?;
            let energy = copy(energy_name, std::ptr::null_mut(), 0, 0, 0);
            CFRelease(energy_name);
            if !energy.is_null() {
                merge(channels, energy, std::ptr::null_mut());
                CFRelease(energy);
            }
            let mut chan = std::ptr::null_mut();
            let sub = create_sub(
                std::ptr::null_mut(),
                channels,
                &mut chan,
                0,
                std::ptr::null_mut(),
            );
            CFRelease(channels);
            if sub.is_null() || chan.is_null() {
                libc::dlclose(handle);
                return None;
            }
            Some(Self {
                handle,
                sub,
                channels: chan,
                previous: std::ptr::null_mut(),
                previous_time: None,
                frequencies: read_gpu_frequencies(),
                sample,
                delta,
                group,
                subgroup,
                channel,
                unit,
                value,
                count,
                state,
                residency,
            })
        }
    }

    fn sample(&mut self) -> Option<AppleGpuStats> {
        unsafe {
            let current = (self.sample)(self.sub, self.channels, std::ptr::null_mut());
            if current.is_null() {
                return None;
            }
            let now = std::time::Instant::now();
            let old = self.previous;
            let elapsed = self
                .previous_time
                .replace(now)
                .map(|t| now.duration_since(t).as_secs_f32());
            self.previous = current;
            if old.is_null() {
                return None;
            }
            let diff = (self.delta)(old, current, std::ptr::null_mut());
            CFRelease(old);
            if diff.is_null() {
                return None;
            }
            let key = make_cf_string("IOReportChannels")?;
            let list = CFDictionaryGetValue(diff, key);
            CFRelease(key);
            if list.is_null() {
                CFRelease(diff);
                return None;
            }
            let mut util = None;
            let mut freq = None;
            let mut power = None;
            let n = CFArrayGetCount(list);
            for i in 0..n {
                let item = CFArrayGetValueAtIndex(list, i) as *mut libc::c_void;
                if item.is_null() {
                    continue;
                }
                let g = cf_string((self.group)(item));
                let sg = cf_string((self.subgroup)(item));
                let ch = cf_string((self.channel)(item));
                if g == "GPU Stats" && sg == "GPU Performance States" && ch == "GPUPH" {
                    let count = (self.count)(item);
                    let mut total = 0i64;
                    let mut active = 0i64;
                    let mut weighted = 0i64;
                    let mut frequency_residency = 0i64;
                    let mut frequency_states = 0usize;
                    let mut offset = 0;
                    for s in 0..count {
                        let residency = (self.residency)(item, s);
                        if residency < 0 {
                            continue;
                        }
                        total += residency;
                        let name = cf_string((self.state)(item, s));
                        if matches!(name.as_str(), "IDLE" | "OFF" | "DOWN") {
                            offset = s + 1;
                        } else {
                            active += residency;
                            frequency_states += 1;
                            if let Some(mhz) = self.frequencies.get((s - offset) as usize) {
                                weighted =
                                    weighted.saturating_add(residency.saturating_mul(*mhz as i64));
                                frequency_residency += residency;
                            }
                        }
                    }
                    if total > 0 {
                        util = Some((active as f32 * 100.0 / total as f32).clamp(0.0, 100.0));
                        if active > 0
                            && frequency_states == self.frequencies.len()
                            && frequency_residency == active
                        {
                            freq = Some((weighted / active) as u32);
                        }
                    }
                }
                if g == "Energy Model" && ch == "GPU Energy" {
                    let value = (self.value)(item, 0);
                    if value >= 0 {
                        if let Some(seconds) = elapsed.filter(|s| *s > 0.0) {
                            let unit = cf_string((self.unit)(item));
                            power = energy_joules(value, &unit).map(|joules| joules / seconds);
                        }
                    }
                }
            }
            CFRelease(diff);
            if util.is_none() && freq.is_none() && power.is_none() {
                None
            } else {
                Some(AppleGpuStats {
                    utilization: util,
                    frequency_mhz: freq,
                    power_watts: power,
                })
            }
        }
    }
}

#[cfg(target_arch = "aarch64")]
impl Drop for AppleGpuReport {
    fn drop(&mut self) {
        unsafe {
            if !self.previous.is_null() {
                CFRelease(self.previous);
            }
            if !self.sub.is_null() {
                CFRelease(self.sub);
            }
            if !self.channels.is_null() {
                CFRelease(self.channels);
            }
            if !self.handle.is_null() {
                libc::dlclose(self.handle);
            }
        }
    }
}

#[cfg(target_arch = "aarch64")]
fn energy_joules(value: i64, unit: &str) -> Option<f32> {
    match unit {
        "nJ" => Some(value as f32 / 1e9),
        "uJ" | "µJ" => Some(value as f32 / 1e6),
        "mJ" => Some(value as f32 / 1e3),
        "J" => Some(value as f32),
        _ => None,
    }
}

#[cfg(target_arch = "aarch64")]
fn read_gpu_frequencies() -> Vec<u32> {
    let text = command_text(&[
        "/usr/sbin/ioreg",
        "-p",
        "IODeviceTree",
        "-r",
        "-n",
        "sgx",
        "-l",
    ]);
    parse_gpu_frequencies(&text)
}

#[cfg(target_arch = "aarch64")]
fn parse_gpu_frequencies(text: &str) -> Vec<u32> {
    let Some(start) = text
        .find("\"perf-states\" = <")
        .map(|p| p + "\"perf-states\" = <".len())
    else {
        return Vec::new();
    };
    let Some(end) = text[start..].find('>').map(|p| start + p) else {
        return Vec::new();
    };
    let bytes: Vec<u8> = text[start..end]
        .as_bytes()
        .chunks(2)
        .filter_map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|s| u8::from_str_radix(s, 16).ok())
        })
        .collect();
    bytes
        .chunks_exact(8)
        .filter_map(|entry| {
            let hz = u32::from_le_bytes(entry[..4].try_into().ok()?);
            (hz > 0).then_some(hz / 1_000_000)
        })
        .collect()
}

#[cfg(target_arch = "aarch64")]
unsafe fn cf_string(value: *const libc::c_void) -> String {
    if value.is_null() {
        return String::new();
    }
    let mut buf = [0i8; 128];
    if CFStringGetCString(
        value,
        buf.as_mut_ptr(),
        buf.len() as libc::c_long,
        0x08000100,
    ) {
        std::ffi::CStr::from_ptr(buf.as_ptr())
            .to_string_lossy()
            .into_owned()
    } else {
        String::new()
    }
}
#[cfg(target_arch = "aarch64")]
unsafe fn make_cf_string(value: &str) -> Option<*mut libc::c_void> {
    let value = std::ffi::CString::new(value).ok()?;
    let result = CFStringCreateWithCString(std::ptr::null_mut(), value.as_ptr(), 0x08000100);
    (!result.is_null()).then_some(result)
}

#[cfg(target_arch = "aarch64")]
extern "C" {
    fn CFStringCreateWithCString(
        a: *const libc::c_void,
        b: *const libc::c_char,
        e: u32,
    ) -> *mut libc::c_void;
    fn CFStringGetCString(
        a: *const libc::c_void,
        b: *mut libc::c_char,
        l: libc::c_long,
        e: u32,
    ) -> bool;
    fn CFRelease(a: *const libc::c_void);
    fn CFDictionaryGetValue(a: *const libc::c_void, k: *const libc::c_void) -> *const libc::c_void;
    fn CFArrayGetCount(a: *const libc::c_void) -> libc::c_long;
    fn CFArrayGetValueAtIndex(a: *const libc::c_void, i: libc::c_long) -> *const libc::c_void;
}

struct AcceleratorStats {
    util_percent: Option<f32>,
    used_memory: Option<u64>,
}

fn read_ioaccelerator_stats() -> AcceleratorStats {
    let text = command_text(&["/usr/sbin/ioreg", "-r", "-d", "1", "-c", "IOAccelerator"]);
    parse_accelerator_stats(&text)
}

fn parse_accelerator_stats(text: &str) -> AcceleratorStats {
    if text
        .lines()
        .filter(|line| line.trim_start().starts_with("+-o "))
        .count()
        != 1
    {
        return AcceleratorStats {
            util_percent: None,
            used_memory: None,
        };
    }
    let util_percent = extract_ioreg_u64(text, "Device Utilization %")
        .filter(|value| *value <= 100)
        .map(|value| value as f32);
    let used_memory = extract_ioreg_u64(text, "In use system memory");

    AcceleratorStats {
        util_percent,
        used_memory,
    }
}

fn is_display_property(trimmed: &str) -> bool {
    [
        "Chipset Model:",
        "Type:",
        "Bus:",
        "Vendor:",
        "Metal Support:",
        "Total Number of Cores:",
        "Displays:",
        "VRAM",
        "PCIe",
        "Device ID:",
    ]
    .iter()
    .any(|p| trimmed.starts_with(p))
}

fn field(line: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    line.strip_prefix(&prefix)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn command_text(cmd: &[&str]) -> String {
    let (bin, args) = match cmd.split_first() {
        Some(v) => v,
        None => return String::new(),
    };
    Command::new(bin)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

fn extract_ioreg_u64(text: &str, key: &str) -> Option<u64> {
    let search = format!("\"{key}\"=");
    let pos = text.find(&search)?;
    let rest = &text[pos + search.len()..];
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    num.parse().ok()
}

fn parse_vendor_id(s: &str) -> u32 {
    if let Some(hex) = s
        .split('(')
        .nth(1)
        .and_then(|x| x.strip_suffix(')'))
        .and_then(|x| x.trim().strip_prefix("0x"))
    {
        if let Ok(id) = u32::from_str_radix(hex, 16) {
            return id;
        }
    }
    let lower = s.to_lowercase();
    if lower.contains("apple") {
        0x106b
    } else if lower.contains("amd") || lower.contains("ati") {
        0x1002
    } else if lower.contains("nvidia") {
        0x10de
    } else if lower.contains("intel") {
        0x8086
    } else {
        0
    }
}

fn parse_hex_u32(s: &str) -> Option<u32> {
    let s = s.trim().strip_prefix("0x").unwrap_or(s.trim());
    u32::from_str_radix(s, 16).ok()
}

fn parse_metal_version(s: &str) -> Option<ApiVersion> {
    let lower = s.to_lowercase();
    let major = lower
        .split("metal ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse::<u32>()
        .ok()?;
    Some(ApiVersion {
        major,
        minor: 0,
        patch: None,
    })
}

fn parse_vram(s: &str) -> Option<u64> {
    let lower = s.to_lowercase();
    let num: f64 = lower
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect::<String>()
        .parse()
        .ok()?;
    if lower.contains("gb") {
        Some((num * 1024.0 * 1024.0 * 1024.0) as u64)
    } else if lower.contains("mb") {
        Some((num * 1024.0 * 1024.0) as u64)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accelerator_samples_preserve_zero_and_unavailable() {
        let stats = parse_accelerator_stats("+-o GPU\n{\"Device Utilization %\"=0,\"In use system memory\"=0,\"Alloc system memory\"=123}");
        assert_eq!(stats.util_percent, Some(0.0));
        assert_eq!(stats.used_memory, Some(0));
        let stats = parse_accelerator_stats(
            "+-o GPU\n{\"Device Utilization %\"=101,\"Alloc system memory\"=123}",
        );
        assert_eq!(stats.util_percent, None);
        assert_eq!(stats.used_memory, None);
        let stats = parse_accelerator_stats("+-o GPU 1\n{\"Device Utilization %\"=30}\n+-o GPU 2");
        assert_eq!(stats.util_percent, None);
        assert_eq!(stats.used_memory, None);
        assert!(parse_metal_version("Supported, Metal GPUFamily Apple 9").is_none());
        assert_eq!(parse_metal_version("Metal 4").unwrap().major, 4);
        #[cfg(target_arch = "aarch64")]
        {
            assert_eq!(energy_joules(1_000_000_000, "nJ"), Some(1.0));
            assert_eq!(energy_joules(1_000_000, "µJ"), Some(1.0));
            assert_eq!(energy_joules(1_000, "mJ"), Some(1.0));
            assert_eq!(energy_joules(1, "J"), Some(1.0));
            assert_eq!(energy_joules(1, "unknown"), None);
            assert_eq!(
                parse_gpu_frequencies("\"perf-states\" = <000000007d0000008078251458020000>"),
                vec![338]
            );
        }
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    #[ignore = "samples native GPU counters"]
    fn native_gpu_sample() {
        let mut cache = <GpuCache as magpie_platform::gpus::GpuCache>::new();
        magpie_platform::gpus::GpuCache::refresh(&mut cache);
        eprintln!(
            "IOReport subscription available: {}",
            cache.apple_report.is_some()
        );
        std::thread::sleep(std::time::Duration::from_secs(1));
        magpie_platform::gpus::GpuCache::refresh(&mut cache);
        for gpu in cache.gpus.values() {
            eprintln!("GPU {:?}: utilization {:?}%, power {:?} W, clock {:?} MHz, shared usage {:?} bytes", gpu.device_name, gpu.utilization_percent, gpu.power_draw_watts, gpu.clock_speed_mhz, gpu.used_shared_memory);
        }
    }
}
