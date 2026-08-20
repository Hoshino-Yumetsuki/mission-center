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

#[repr(C)]
#[derive(Default, Copy, Clone)]
struct XswUsage {
    total: u64,
    avail: u64,
    used: u64,
    pagesize: u32,
    encrypted: libc::c_int,
}


fn refresh_memory(memory: &mut Memory) {
    if let Some(total) = sysctl_u64("hw.memsize") {
        memory.mem_total = total;
    }

    let (stats, page_size) = match read_vm_statistics() {
        Some(value) => value,
        None => return,
    };
    let (swap_total, swap_used) = read_swap_usage().unwrap_or((0, 0));
    let page = page_size as u64;
    let pages_free = stats.free_count as u64;
    let pages_inactive = stats.inactive_count as u64;
    let pages_wired = stats.wire_count as u64;
    let pages_purgeable = stats.purgeable_count as u64;
    let pages_speculative = stats.speculative_count as u64;
    let pages_file_backed = stats.external_page_count as u64;
    let pages_anonymous = stats.internal_page_count as u64;
    let pages_compressor = stats.compressor_page_count as u64;
    let pages_stored_in_compressor = stats.total_uncompressed_pages_in_compressor as u64;

    let in_use = (pages_anonymous + pages_wired + pages_compressor) * page;
    let cached = (pages_file_backed + pages_purgeable) * page;
    let real_free = pages_free.saturating_sub(pages_speculative);
    let available = (real_free + pages_inactive) * page;
    let committed =
        (pages_anonymous + pages_wired + pages_stored_in_compressor) * page + swap_used;

    memory.mem_free = real_free * page;
    memory.mem_available = available;
    memory.active = in_use;
    memory.inactive = pages_inactive * page;
    memory.cached = cached;
    memory.anon_pages = pages_anonymous * page;
    memory.active_anon = pages_anonymous * page;
    memory.active_file = pages_file_backed * page;
    memory.unevictable = pages_wired * page;
    memory.m_locked = pages_wired * page;
    memory.swap_total = swap_total;
    memory.swap_free = swap_total.saturating_sub(swap_used);
    memory.committed = committed;
    memory.zswap = pages_compressor * page;
    memory.zswapped = pages_stored_in_compressor * page;
}

fn read_vm_statistics() -> Option<(libc::vm_statistics64, u32)> {
    unsafe {
        type Host = mach2::mach_types::host_t;
        type Count = mach2::message::mach_msg_type_number_t;
        extern "C" {
            fn mach_host_self() -> Host;
            fn host_page_size(host: Host, size: *mut u32) -> i32;
            fn host_statistics64(host: Host, flavor: i32, info: *mut i32, count: *mut Count) -> i32;
        }
        let host = mach_host_self();
        let mut page_size = 0u32;
        let mut stats: libc::vm_statistics64 = std::mem::zeroed();
        let mut count = (std::mem::size_of::<libc::vm_statistics64>()
            / std::mem::size_of::<u32>()) as Count;
        let expected_count = count;
        if host_page_size(host, &mut page_size) != 0
            || page_size == 0
            || host_statistics64(
                host,
                4,
                &mut stats as *mut libc::vm_statistics64 as *mut i32,
                &mut count,
            ) != 0
            || count < expected_count
        {
            None
        } else {
            Some((stats, page_size))
        }
    }
}

fn read_swap_usage() -> Option<(u64, u64)> {
    unsafe {
        let name = std::ffi::CString::new("vm.swapusage").ok()?;
        let mut usage = XswUsage::default();
        let mut size = std::mem::size_of::<XswUsage>();
        if libc::sysctlbyname(
            name.as_ptr(),
            &mut usage as *mut XswUsage as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        ) == 0 && size >= std::mem::size_of::<XswUsage>()
        {
            Some((usage.total, usage.used))
        } else {
            None
        }
    }
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
