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
    let (Some(total), Some((stats, page_size))) = (sysctl_u64("hw.memsize"), read_vm_statistics())
    else {
        *memory = Memory::default();
        return;
    };
    memory.mem_total = total;
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

    let real_free = pages_free.saturating_sub(pages_speculative) * page;
    let in_use =
        (pages_anonymous.saturating_sub(pages_purgeable) + pages_wired + pages_compressor) * page;
    memory.mem_free = real_free;
    memory.mem_available = total.saturating_sub(in_use).max(real_free);
    memory.active = stats.active_count as u64 * page;
    memory.inactive = pages_inactive * page;
    memory.cached = (pages_file_backed + pages_purgeable) * page;
    memory.anon_pages = pages_anonymous * page;
    memory.unevictable = pages_wired * page;
    if let Some((total, used)) = read_swap_usage() {
        memory.swap_total = total;
        memory.swap_free = total.saturating_sub(used);
    } else {
        memory.swap_total = 0;
        memory.swap_free = 0;
    }
    memory.zswap = pages_compressor * page;
    memory.zswapped = pages_stored_in_compressor * page;
}

fn read_vm_statistics() -> Option<(libc::vm_statistics64, u32)> {
    unsafe {
        extern "C" {
            fn mach_host_self() -> libc::mach_port_t;
        }
        let host = mach_host_self();
        let page_size = libc::sysconf(libc::_SC_PAGESIZE);
        let mut stats: libc::vm_statistics64 = std::mem::zeroed();
        let mut count = libc::HOST_VM_INFO64_COUNT;
        let result = libc::host_statistics64(
            host,
            libc::HOST_VM_INFO64,
            (&raw mut stats).cast(),
            &mut count,
        );
        mach2::mach_port::mach_port_deallocate(mach2::traps::mach_task_self(), host);
        // Older kernels return the prefix they implement; we only need the compressor fields.
        let needed = std::mem::offset_of!(
            libc::vm_statistics64,
            total_uncompressed_pages_in_compressor
        ) + 8;
        (result == 0 && page_size > 0 && (count as usize) * 4 >= needed)
            .then_some((stats, page_size as u32))
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
        ) == 0
            && size >= std::mem::size_of::<XswUsage>()
        {
            Some((usage.total, usage.used))
        } else {
            None
        }
    }
}

fn load_memory_devices(total: u64) -> Option<MemoryDevice> {
    // Apple Silicon exposes the type of its unified memory, not DIMM slots.
    let output = std::process::Command::new("/usr/sbin/system_profiler")
        .args(["SPMemoryDataType", "-xml", "-detailLevel", "mini"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let data = plist::Value::from_reader_xml(&output.stdout[..]).ok()?;
    let item = data
        .as_array()?
        .first()?
        .as_dictionary()?
        .get("_items")?
        .as_array()?
        .first()?
        .as_dictionary()?;
    let ram_type = item.get("dimm_type")?.as_string()?.to_owned();
    Some(MemoryDevice {
        size: total,
        ram_type,
        ..Default::default()
    })
}

impl magpie_platform::memory::MemoryCache for MemoryCache {
    fn new() -> Self {
        let mut memory = Memory::default();
        refresh_memory(&mut memory);
        let devices = load_memory_devices(memory.mem_total).into_iter().collect();
        Self { memory, devices }
    }

    fn refresh(&mut self) {
        refresh_memory(&mut self.memory);
    }

    fn memory(&self) -> &Memory {
        &self.memory
    }

    fn devices(&self) -> &[MemoryDevice] {
        &self.devices
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_memory_matches_hardware_size() {
        let mut memory = Memory::default();
        refresh_memory(&mut memory);
        assert_eq!(Some(memory.mem_total), sysctl_u64("hw.memsize"));
        assert!(
            memory.mem_available >= memory.mem_free && memory.mem_available <= memory.mem_total
        );
        assert!(memory.mem_available < memory.mem_total);
        assert!(memory.swap_free <= memory.swap_total);
    }
}
