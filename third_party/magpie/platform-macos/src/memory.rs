/* src/memory.rs
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

use magpie_platform::memory::{Memory, MemoryDevice};

use crate::util::sysctl_u64;

pub struct MemoryCache {
    memory: Memory,
    devices: Vec<MemoryDevice>,
}

fn refresh_memory(memory: &mut Memory) {
    if let Some(total) = sysctl_u64("hw.memsize") {
        memory.mem_total = total;
    }

    let Ok(vm_stat_output) = std::process::Command::new("vm_stat").output() else {
        return;
    };
    let vm_stat_str = String::from_utf8_lossy(&vm_stat_output.stdout);

    let mut page_size: u64 = 4096;
    let mut pages_free: u64 = 0;
    let mut pages_active: u64 = 0;
    let mut pages_inactive: u64 = 0;
    let mut pages_wired: u64 = 0;
    let mut pages_purgeable: u64 = 0;
    let mut pages_speculative: u64 = 0;
    let mut pages_anonymous: u64 = 0;
    let mut pages_file_backed: u64 = 0;
    let mut pages_compressor: u64 = 0;
    let mut pages_stored_in_compressor: u64 = 0;

    for line in vm_stat_str.lines() {
        if line.starts_with("Mach Virtual Memory Statistics:") {
            if let Some(ps) = line.split("page size of ").nth(1) {
                if let Some(ps) = ps.split(" bytes").next() {
                    page_size = ps.trim().parse::<u64>().unwrap_or(4096);
                }
            }
        }
        let mut parts = line.splitn(2, ':');
        let key = parts.next().unwrap_or("").trim();
        let val = parts
            .next()
            .unwrap_or("")
            .trim()
            .trim_end_matches('.')
            .trim()
            .parse::<u64>()
            .unwrap_or(0);
        match key {
            "Pages free" => pages_free = val,
            "Pages active" => pages_active = val,
            "Pages inactive" => pages_inactive = val,
            "Pages wired down" => pages_wired = val,
            "Pages purgeable" => pages_purgeable = val,
            "Pages speculative" => pages_speculative = val,
            "Anonymous pages" => pages_anonymous = val,
            "File-backed pages" => pages_file_backed = val,
            "Pages occupied by compressor" => pages_compressor = val,
            "Pages stored in compressor" => pages_stored_in_compressor = val,
            _ => {}
        }
    }

    let mut swap_used: u64 = 0;
    let mut swap_total: u64 = 0;
    if let Ok(out) = std::process::Command::new("sysctl")
        .arg("vm.swapusage")
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout);
        // "vm.swapusage: total = 1024.00M  used = 12.50M  free = 1011.50M ..."
        for part in s.split_whitespace() {
            if let Some(stripped) = part.strip_suffix('M') {
                if let Ok(v) = stripped.parse::<f64>() {
                    let bytes = (v * 1024.0 * 1024.0) as u64;
                    if swap_total == 0 {
                        swap_total = bytes;
                    } else if swap_used == 0 {
                        swap_used = bytes;
                    }
                }
            }
        }
    }

    let in_use = (pages_anonymous + pages_wired + pages_compressor) * page_size;
    let cached = (pages_file_backed + pages_purgeable) * page_size;
    let real_free = pages_free.saturating_sub(pages_speculative);
    let available = (real_free + pages_inactive) * page_size;
    let committed =
        (pages_anonymous + pages_wired + pages_stored_in_compressor) * page_size + swap_used;

    memory.mem_free = real_free * page_size;
    memory.mem_available = available;
    memory.active = in_use;
    memory.inactive = pages_inactive * page_size;
    memory.cached = cached;
    memory.anon_pages = pages_anonymous * page_size;
    memory.active_anon = pages_anonymous * page_size;
    memory.active_file = pages_file_backed * page_size;
    memory.unevictable = pages_wired * page_size;
    memory.m_locked = pages_wired * page_size;
    memory.swap_total = swap_total;
    memory.swap_free = swap_total.saturating_sub(swap_used);
    memory.committed = committed;
    // Map compressor occupancy into zswap-like fields for UI parity.
    memory.zswap = pages_compressor * page_size;
    memory.zswapped = pages_stored_in_compressor * page_size;
    let _ = pages_active; // used via in_use composition above
}

fn load_memory_devices() -> (Vec<MemoryDevice>, u64) {
    // Apple Silicon reports aggregated memory via SPMemoryDataType (no DIMM slots).
    let text = std::process::Command::new("/usr/sbin/system_profiler")
        .args(["SPMemoryDataType", "-detailLevel", "mini"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();

    let mut ram_type = String::new();
    let mut size_bytes = 0u64;
    let mut manufacturer = String::new();

    for line in text.lines() {
        let t = line.trim();
        if let Some(v) = t.strip_prefix("Type:") {
            ram_type = v.trim().to_string();
        } else if let Some(v) = t.strip_prefix("Memory:") {
            size_bytes = parse_size_bytes(v.trim()).unwrap_or(0);
        } else if let Some(v) = t.strip_prefix("Manufacturer:") {
            manufacturer = v.trim().to_string();
        }
    }

    if size_bytes == 0 {
        size_bytes = sysctl_u64("hw.memsize").unwrap_or(0);
    }

    if size_bytes == 0 && ram_type.is_empty() {
        return (Vec::new(), 0);
    }

    let locator = if manufacturer.is_empty() {
        "System".to_string()
    } else {
        manufacturer
    };

    (
        vec![MemoryDevice {
            size: size_bytes,
            form_factor: String::new(), // soldered / unknown — hide in UI
            locator,
            bank_locator: String::new(),
            ram_type,
            speed: 0, // MT/s not exposed — hide in UI
            rank: 0,
        }],
        1,
    )
}

fn parse_size_bytes(s: &str) -> Option<u64> {
    let lower = s.to_lowercase();
    let num: f64 = lower
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect::<String>()
        .parse()
        .ok()?;
    if lower.contains("tb") {
        Some((num * 1024.0 * 1024.0 * 1024.0 * 1024.0) as u64)
    } else if lower.contains("gb") {
        Some((num * 1024.0 * 1024.0 * 1024.0) as u64)
    } else if lower.contains("mb") {
        Some((num * 1024.0 * 1024.0) as u64)
    } else {
        Some(num as u64)
    }
}

impl magpie_platform::memory::MemoryCache for MemoryCache {
    fn new() -> Self {
        let mut memory = Memory::default();
        refresh_memory(&mut memory);
        let (devices, max_devices) = load_memory_devices();
        memory.max_devices = max_devices;
        Self { memory, devices }
    }

    fn refresh(&mut self) {
        refresh_memory(&mut self.memory);
        // Device topology is static; keep cached devices / max_devices.
        self.memory.max_devices = self.devices.len() as u64;
    }

    fn memory(&self) -> &Memory {
        &self.memory
    }

    fn devices(&self) -> &[MemoryDevice] {
        &self.devices
    }
}
