/* sys_info_v2/mem_info.rs
 *
 * Copyright 2023 Romeo Calota
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

#[derive(Default, Debug, Eq, PartialEq)]
pub struct MemoryDevice {
    pub size: usize,
    pub form_factor: String,
    pub locator: String,
    pub bank_locator: String,
    pub ram_type: String,
    pub speed: usize,
    pub rank: u8,
}

#[derive(Default, Debug, Copy, Clone, Eq, PartialEq)]
pub struct MemInfo {
    pub mem_total: usize,
    pub mem_free: usize,
    pub mem_available: usize,
    pub buffers: usize,
    pub cached: usize,
    pub swap_cached: usize,
    pub active: usize,
    pub inactive: usize,
    pub active_anon: usize,
    pub inactive_anon: usize,
    pub active_file: usize,
    pub inactive_file: usize,
    pub unevictable: usize,
    pub m_locked: usize,
    pub swap_total: usize,
    pub swap_free: usize,
    pub zswap: usize,
    pub zswapped: usize,
    pub dirty: usize,
    pub writeback: usize,
    pub anon_pages: usize,
    pub mapped: usize,
    pub sh_mem: usize,
    pub k_reclaimable: usize,
    pub slab: usize,
    pub s_reclaimable: usize,
    pub s_unreclaim: usize,
    pub kernel_stack: usize,
    pub page_tables: usize,
    pub sec_page_tables: usize,
    pub nfs_unstable: usize,
    pub bounce: usize,
    pub writeback_tmp: usize,
    pub commit_limit: usize,
    pub committed: usize,
    pub vmalloc_total: usize,
    pub vmalloc_used: usize,
    pub vmalloc_chunk: usize,
    pub percpu: usize,
    pub hardware_corrupted: usize,
    pub anon_huge_pages: usize,
    pub shmem_huge_pages: usize,
    pub shmem_pmd_mapped: usize,
    pub file_huge_pages: usize,
    pub file_pmd_mapped: usize,
    pub cma_total: usize,
    pub cma_free: usize,
    pub huge_pages_total: usize,
    pub huge_pages_free: usize,
    pub huge_pages_rsvd: usize,
    pub huge_pages_surp: usize,
    pub hugepagesize: usize,
    pub hugetlb: usize,
    pub direct_map4k: usize,
    pub direct_map2m: usize,
    pub direct_map1g: usize,
}

#[cfg(target_os = "linux")]
impl MemInfo {
    pub fn load() -> Option<Self> {
        use gtk::glib::*;

        let meminfo = if let Ok(output) = cmd!("cat /proc/meminfo").output() {
            if output.stderr.len() > 0 {
                g_critical!(
                    "MissionCenter::MemInfo",
                    "Failed to refresh memory information, host command execution failed: {}",
                    String::from_utf8_lossy(output.stderr.as_slice())
                );
                return None;
            }

            String::from_utf8_lossy(output.stdout.as_slice()).into_owned()
        } else {
            g_critical!(
                "MissionCenter::MemInfo",
                "Failed to refresh memory information, host command execution failed"
            );

            return None;
        };

        let mut this = Self::default();

        for line in meminfo.trim().lines() {
            let mut split = line.split_whitespace();
            let key = split.next().map_or("", |s| s);
            let value = split
                .next()
                .map_or("", |s| s)
                .parse::<usize>()
                .map_or(0, |v| v)
                * 1024;

            match key {
                "MemTotal:" => this.mem_total = value,
                "MemFree:" => this.mem_free = value,
                "MemAvailable:" => this.mem_available = value,
                "Buffers:" => this.buffers = value,
                "Cached:" => this.cached = value,
                "SwapCached:" => this.swap_cached = value,
                "Active:" => this.active = value,
                "Inactive:" => this.inactive = value,
                "Active(anon):" => this.active_anon = value,
                "Inactive(anon):" => this.inactive_anon = value,
                "Active(file):" => this.active_file = value,
                "Inactive(file):" => this.inactive_file = value,
                "Unevictable:" => this.unevictable = value,
                "Mlocked:" => this.m_locked = value,
                "SwapTotal:" => this.swap_total = value,
                "SwapFree:" => this.swap_free = value,
                "ZSwap:" => this.zswap = value,
                "ZSwapTotal:" => this.zswapped = value,
                "Dirty:" => this.dirty = value,
                "Writeback:" => this.writeback = value,
                "AnonPages:" => this.anon_pages = value,
                "Mapped:" => this.mapped = value,
                "Shmem:" => this.sh_mem = value,
                "KReclaimable:" => this.k_reclaimable = value,
                "Slab:" => this.slab = value,
                "SReclaimable:" => this.s_reclaimable = value,
                "SUnreclaim:" => this.s_unreclaim = value,
                "KernelStack:" => this.kernel_stack = value,
                "PageTables:" => this.page_tables = value,
                "SecMemTables:" => this.sec_page_tables = value,
                "NFS_Unstable:" => this.nfs_unstable = value,
                "Bounce:" => this.bounce = value,
                "WritebackTmp:" => this.writeback_tmp = value,
                "CommitLimit:" => this.commit_limit = value,
                "Committed_AS:" => this.committed = value,
                "VmallocTotal:" => this.vmalloc_total = value,
                "VmallocUsed:" => this.vmalloc_used = value,
                "VmallocChunk:" => this.vmalloc_chunk = value,
                "Percpu:" => this.percpu = value,
                "HardwareCorrupted:" => this.hardware_corrupted = value,
                "AnonHugePages:" => this.anon_huge_pages = value,
                "ShmemHugePages:" => this.shmem_huge_pages = value,
                "ShmemPmdMapped:" => this.shmem_pmd_mapped = value,
                "FileHugePages:" => this.file_huge_pages = value,
                "FilePmdMapped:" => this.file_pmd_mapped = value,
                "CmaTotal:" => this.cma_total = value,
                "CmaFree:" => this.cma_free = value,
                "HugePages_Total:" => this.huge_pages_total = value / 1024,
                "HugePages_Free:" => this.huge_pages_free = value / 1024,
                "HugePages_Rsvd:" => this.huge_pages_rsvd = value / 1024,
                "HugePages_Surp:" => this.huge_pages_surp = value / 1024,
                "Hugepagesize:" => this.hugepagesize = value,
                "Hugetlb:" => this.hugetlb = value,
                "DirectMap4k:" => this.direct_map4k = value,
                "DirectMap2M:" => this.direct_map2m = value,
                "DirectMap1G:" => this.direct_map1g = value,
                _ => (),
            }
        }

        Some(this)
    }

    pub fn load_memory_device_info() -> Option<Vec<MemoryDevice>> {
        use gtk::glib::*;
        use std::process::*;

        let is_flatpak = *super::IS_FLATPAK;
        let mut cmd = if !is_flatpak {
            let mut cmd = Command::new("udevadm");
            cmd.arg("info")
                .arg("-q")
                .arg("property")
                .arg("-p")
                .arg("/sys/devices/virtual/dmi/id");
            cmd.env_remove("LD_PRELOAD");
            cmd
        } else {
            let mut cmd =
                cmd_flatpak_host!("udevadm info -q property -p /sys/devices/virtual/dmi/id");
            cmd.env_remove("LD_PRELOAD");
            cmd
        };

        let cmd_output = match cmd.output() {
            Ok(output) => {
                if output.stderr.len() > 0 {
                    g_critical!(
                    "MissionCenter::SysInfo",
                    "Failed to read memory device information, host command execution failed: {}",
                    std::str::from_utf8(output.stderr.as_slice()).unwrap_or("Unknown error")
                );
                    return None;
                }

                match std::str::from_utf8(output.stdout.as_slice()) {
                    Ok(out) => out.to_owned(),
                    Err(err) => {
                        g_critical!(
                            "MissionCenter::SysInfo",
                            "Failed to read memory device information, host command execution failed: {:?}",
                            err
                        );
                        return None;
                    }
                }
            }
            Err(err) => {
                g_critical!(
                    "MissionCenter::SysInfo",
                    "Failed to read memory device information, host command execution failed: {:?}",
                    err
                );
                return None;
            }
        };

        let mut result = vec![];

        let mut cmd_output_str = cmd_output.as_str();
        let mut cmd_output_str_index = 0;
        let mut module_index = 0;
        let mut speed_fallback = 0;

        loop {
            if cmd_output_str_index >= cmd_output_str.len() {
                break;
            }

            let to_parse = cmd_output_str.trim();
            let mem_dev_string = format!("MEMORY_DEVICE_{}_", module_index);
            let mem_dev_str = mem_dev_string.as_str();
            cmd_output_str_index = match to_parse.find(mem_dev_str) {
                None => {
                    break;
                }
                Some(cmd_output_str_index) => cmd_output_str_index,
            };
            cmd_output_str_index += mem_dev_str.len();
            module_index += 1;
            if cmd_output_str_index < cmd_output_str.len() {
                cmd_output_str = cmd_output_str[cmd_output_str_index..].trim();
            }

            let mut mem_dev = Some(MemoryDevice::default());

            for line in to_parse[cmd_output_str_index..].trim().lines() {
                let mut split = line.trim().split("=");
                let mut key = split.next().map_or("", |s| s).trim();
                let value = split.next().map_or("", |s| s).trim();

                key = key.strip_prefix(mem_dev_str).unwrap_or(key);

                let md = match mem_dev.as_mut() {
                    Some(mem_dev) => mem_dev,
                    None => {
                        break;
                    }
                };

                match key {
                    "PRESENT" => {
                        if value == "0" {
                            #[allow(dropping_references)]
                            drop(md);
                            mem_dev = None;
                            break;
                        }
                    }
                    "SIZE" => md.size = value.parse::<usize>().map_or(0, |s| s),
                    "FORM_FACTOR" => md.form_factor = value.to_owned(),
                    "LOCATOR" => md.locator = value.to_owned(),
                    "BANK_LOCATOR" => md.bank_locator = value.to_owned(),
                    "TYPE" => md.ram_type = value.to_owned(),
                    "SPEED_MTS" => speed_fallback = value.parse::<usize>().map_or(0, |s| s),
                    "CONFIGURED_SPEED_MTS" => md.speed = value.parse::<usize>().map_or(0, |s| s),
                    "RANK" => md.rank = value.parse::<u8>().map_or(0, |s| s),
                    _ => (),
                }
            }

            match mem_dev {
                Some(mut mem_dev) => {
                    if mem_dev.speed == 0 {
                        mem_dev.speed = speed_fallback;
                    }
                    result.push(mem_dev);
                }
                _ => {}
            }
        }

        Some(result)
    }
}

#[cfg(target_os = "macos")]
impl MemInfo {
    pub fn load() -> Option<Self> {
        let mut this = Self::default();

        unsafe {
            let mut mem_size: u64 = 0;
            let mut size = std::mem::size_of::<u64>();
            let name = b"hw.memsize\0";
            if libc::sysctlbyname(
                name.as_ptr() as *const libc::c_char,
                &mut mem_size as *mut u64 as *mut libc::c_void,
                &mut size,
                std::ptr::null_mut(),
                0,
            ) == 0
            {
                this.mem_total = mem_size as usize;
            }
        }

        let vm_stat_output = std::process::Command::new("vm_stat").output().ok()?;
        let vm_stat_str = String::from_utf8_lossy(&vm_stat_output.stdout);

        let mut page_size: usize = 4096;
        let mut pages_free: usize = 0;
        let mut pages_active: usize = 0;
        let mut pages_inactive: usize = 0;
        let mut pages_wired: usize = 0;
        let mut pages_purgeable: usize = 0;
        let mut pages_speculative: usize = 0;
        let mut pages_anonymous: usize = 0;
        let mut pages_file_backed: usize = 0;
        let mut pages_compressor: usize = 0;
        let mut pages_stored_in_compressor: usize = 0;
        let mut swap_used: usize = 0;
        let mut swap_total: usize = 0;

        for line in vm_stat_str.lines() {
            if line.starts_with("Mach Virtual Memory Statistics:") {
                if let Some(ps) = line.split("page size of ").nth(1) {
                    if let Some(ps) = ps.split(" bytes").next() {
                        page_size = ps.trim().parse::<usize>().unwrap_or(4096);
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
                .parse::<usize>()
                .unwrap_or(0);
            match key {
                "Pages free"                    => pages_free = val,
                "Pages active"                  => pages_active = val,
                "Pages inactive"                => pages_inactive = val,
                "Pages wired down"              => pages_wired = val,
                "Pages purgeable"               => pages_purgeable = val,
                "Pages speculative"             => pages_speculative = val,
                "Anonymous pages"               => pages_anonymous = val,
                "File-backed pages"             => pages_file_backed = val,
                "Pages occupied by compressor"  => pages_compressor = val,
                "Pages stored in compressor"    => pages_stored_in_compressor = val,
                _ => {}
            }
        }

        let sysctl_swap = std::process::Command::new("sysctl")
            .arg("vm.swapusage")
            .output()
            .ok();
        if let Some(out) = sysctl_swap {
            let s = String::from_utf8_lossy(&out.stdout);
            for part in s.split_whitespace() {
                if let Some(stripped) = part.strip_suffix("M") {
                    if let Ok(v) = stripped.parse::<f64>() {
                        let bytes = (v * 1024.0 * 1024.0) as usize;
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
        let committed = (pages_anonymous + pages_wired + pages_stored_in_compressor) * page_size
            + swap_used;

        this.mem_free      = real_free * page_size;
        this.active        = in_use;
        this.inactive      = pages_inactive * page_size;
        this.cached        = cached;
        this.mem_available = available;
        this.committed     = committed;
        this.dirty         = 0;
        this.swap_total    = swap_total;
        this.swap_free     = swap_total.saturating_sub(swap_used);

        Some(this)
    }

    pub fn load_memory_device_info() -> Option<Vec<MemoryDevice>> {
        let output = std::process::Command::new("system_profiler")
            .arg("SPMemoryDataType")
            .arg("-json")
            .output()
            .ok()?;

        let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
        let items = json
            .get("SPMemoryDataType")
            .and_then(|x| x.as_array())?;

        let mut result = vec![];
        for item in items {
            let size_str = item.get("dimm_size").and_then(|x| x.as_str()).unwrap_or("0");
            let size_mb = size_str
                .trim_end_matches(" MB")
                .trim_end_matches(" GB")
                .parse::<usize>()
                .unwrap_or(0);
            let size = if size_str.ends_with(" GB") {
                size_mb * 1024 * 1024 * 1024
            } else {
                size_mb * 1024 * 1024
            };

            let speed_str = item.get("dimm_speed").and_then(|x| x.as_str()).unwrap_or("0");
            let speed = speed_str
                .trim_end_matches(" MHz")
                .parse::<usize>()
                .unwrap_or(0);

            result.push(MemoryDevice {
                size,
                form_factor: item.get("dimm_type").and_then(|x| x.as_str()).unwrap_or("").to_owned(),
                locator: item.get("_name").and_then(|x| x.as_str()).unwrap_or("").to_owned(),
                bank_locator: String::new(),
                ram_type: item.get("dimm_type").and_then(|x| x.as_str()).unwrap_or("").to_owned(),
                speed,
                rank: 0,
            });
        }

        Some(result)
    }
}
