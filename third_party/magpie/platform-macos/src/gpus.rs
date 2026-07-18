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

use crate::util::sysctl_u64;

pub struct GpuCache {
    gpus: HashMap<String, Gpu>,
    static_loaded: bool,
}

impl magpie_platform::gpus::GpuCache for GpuCache {
    fn new() -> Self {
        Self {
            gpus: HashMap::new(),
            static_loaded: false,
        }
    }

    fn refresh(&mut self) {
        if !self.static_loaded {
            self.gpus = enumerate_static_gpus();
            self.static_loaded = true;
        }
        refresh_dynamic(&mut self.gpus);
    }

    fn cached_entries(&self) -> &HashMap<String, Gpu> {
        &self.gpus
    }
}

fn enumerate_static_gpus() -> HashMap<String, Gpu> {
    let total_mem = sysctl_u64("hw.memsize");
    let text = command_text(&["/usr/sbin/system_profiler", "SPDisplaysDataType"]);

    let mut gpus = HashMap::new();
    let mut idx = 0u32;
    let mut name: Option<String> = None;
    let mut vendor_id = 0u32;
    let mut device_id = 0u32;
    let mut metal: Option<ApiVersion> = None;
    let mut total_memory = total_mem;

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
        *total_memory = total_mem;
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
        } else if let Some(v) = field(trimmed, "VRAM (Total)")
            .or_else(|| field(trimmed, "VRAM (Dynamic, Max)"))
            .or_else(|| field(trimmed, "VRAM"))
        {
            total_memory = parse_vram(&v).or(total_mem);
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

    if gpus.is_empty() {
        gpus.insert(
            "gpu-0".into(),
            Gpu {
                id: "gpu-0".into(),
                device_name: Some("Unknown GPU".into()),
                vendor_id: 0,
                device_id: 0,
                total_memory: total_mem,
                encode_decode_shared: true,
                ..Default::default()
            },
        );
    }

    gpus
}

fn refresh_dynamic(gpus: &mut HashMap<String, Gpu>) {
    let stats = read_ioaccelerator_stats();
    for gpu in gpus.values_mut() {
        if stats.util_percent > 0.0 {
            gpu.utilization_percent = Some(stats.util_percent);
        }
        if stats.used_memory > 0 {
            gpu.used_memory = Some(stats.used_memory);
            gpu.used_shared_memory = Some(stats.used_memory);
            if let Some(total) = gpu.total_memory {
                gpu.total_shared_memory = Some(total);
            }
        }
    }
}

struct AcceleratorStats {
    util_percent: f32,
    used_memory: u64,
}

fn read_ioaccelerator_stats() -> AcceleratorStats {
    let text = command_text(&["/usr/sbin/ioreg", "-r", "-d", "1", "-c", "IOAccelerator"]);

    let util_percent = extract_ioreg_u64(&text, "Device Utilization %")
        .or_else(|| extract_ioreg_u64(&text, "Renderer Utilization %"))
        .unwrap_or(0) as f32;
    let used_memory = extract_ioreg_u64(&text, "In use system memory")
        .or_else(|| extract_ioreg_u64(&text, "Alloc system memory"))
        .unwrap_or(0);

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
        .split_whitespace()
        .find_map(|tok| tok.parse::<u32>().ok())
        .or_else(|| lower.contains("metal").then_some(1))?;
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
        Some(num as u64)
    }
}
