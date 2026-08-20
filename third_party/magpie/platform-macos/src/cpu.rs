/* src/cpu.rs
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

use magpie_platform::cpu::Cpu;

use crate::util::{sysctl_string, sysctl_u32, sysctl_u64, uptime_seconds};
#[cfg(target_arch = "aarch64")]
use crate::sensors::apple_silicon_temperature;
#[cfg(not(target_arch = "aarch64"))]
use crate::fan::smc_cpu_temperature;

struct CpuTicks {
    user: u64,
    system: u64,
    idle: u64,
    nice: u64,
}

pub struct CpuCache {
    cpu: Cpu,
    prev_ticks: Vec<CpuTicks>,
    static_loaded: bool,
}

impl CpuCache {
    fn load_static(&mut self) {
        let name = sysctl_string("machdep.cpu.brand_string")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Unknown CPU".to_string());

        let logical = sysctl_u32("hw.logicalcpu").unwrap_or(1) as usize;
        let packages = sysctl_u32("hw.packages").unwrap_or(1).max(1);

        let info = &mut self.cpu;
        info.name = Some(name);
        info.socket_count = Some(packages);
        // Apple Silicon: hw.cpufrequency is often missing or 0 — treat as unavailable.
        info.base_freq_khz = sysctl_u64("hw.cpufrequency")
            .map(|f| f / 1000)
            .filter(|&f| f > 0);

        let l1i = sysctl_u64("hw.l1icachesize").unwrap_or(0);
        let l1d = sysctl_u64("hw.l1dcachesize").unwrap_or(0);
        info.l1_combined_cache_bytes = if l1i + l1d > 0 {
            Some(l1i + l1d)
        } else {
            None
        };
        info.l2_cache_bytes = sysctl_u64("hw.l2cachesize");
        info.l3_cache_bytes = sysctl_u64("hw.l3cachesize");
        info.l4_cache_bytes = None;

        let is_vm = sysctl_u32("kern.hv_vmm_present").map(|v| v != 0);
        info.is_virtual_machine = is_vm;
        info.virtualization_technology = if is_vm == Some(true) {
            Some("Hypervisor".to_string())
        } else {
            None
        };

        info.core_usage_percent.resize(logical, 0.0);
        info.core_kernel_usage_percent.resize(logical, 0.0);
        self.prev_ticks.clear();
        self.static_loaded = true;
    }

    fn refresh_dynamic(&mut self) {
        let cpu_count = self.cpu.core_usage_percent.len().max(1);
        let new_ticks = read_per_cpu_ticks(cpu_count);

        if self.prev_ticks.len() != new_ticks.len() {
            self.prev_ticks = new_ticks;
            self.cpu.total_usage_percent = 0.0;
            self.cpu.kernel_usage_percent = 0.0;
            self.cpu.core_usage_percent.fill(0.0);
            self.cpu.core_kernel_usage_percent.fill(0.0);
        } else {
            let n = new_ticks.len().min(self.cpu.core_usage_percent.len());
            for i in 0..n {
                let prev = &self.prev_ticks[i];
                let curr = &new_ticks[i];
                let total_delta = (curr.user + curr.system + curr.idle + curr.nice)
                    .saturating_sub(prev.user + prev.system + prev.idle + prev.nice)
                    as f32;
                let total_delta = total_delta.max(1.0);
                let user_delta = curr.user.saturating_sub(prev.user) as f32;
                let sys_delta = curr.system.saturating_sub(prev.system) as f32;
                self.cpu.core_usage_percent[i] = (user_delta + sys_delta) / total_delta * 100.0;
                self.cpu.core_kernel_usage_percent[i] = sys_delta / total_delta * 100.0;
            }
            let n = n.max(1) as f32;
            self.cpu.total_usage_percent =
                self.cpu.core_usage_percent.iter().sum::<f32>() / n;
            self.cpu.kernel_usage_percent =
                self.cpu.core_kernel_usage_percent.iter().sum::<f32>() / n;
            self.prev_ticks = new_ticks;
        }

        self.cpu.current_frequency_mhz = sysctl_u64("hw.cpufrequency")
            .or_else(|| sysctl_u64("hw.cpufrequency_max"))
            .map(|f| f / 1_000_000)
            .filter(|&f| f > 0)
            .unwrap_or(0);

        self.cpu.temperature_celsius = {
            #[cfg(target_arch = "aarch64")]
            {
                apple_silicon_temperature()
            }
            #[cfg(not(target_arch = "aarch64"))]
            {
                smc_cpu_temperature()
            }
        };
        self.cpu.frequency_driver = None;
        self.cpu.frequency_governor = None;
        self.cpu.power_preference = None;
        self.cpu.power_draw_w = None;

        let (procs, threads) = process_and_thread_count();
        self.cpu.total_process_count = procs;
        self.cpu.total_thread_count = threads;
        self.cpu.total_handle_count = 0;
        self.cpu.uptime_seconds = uptime_seconds();
    }
}

impl magpie_platform::cpu::CpuCache for CpuCache {
    fn new() -> Self {
        let logical = sysctl_u32("hw.logicalcpu").unwrap_or(1) as usize;
        let mut cpu = Cpu::default();
        cpu.core_usage_percent.resize(logical, 0.0);
        cpu.core_kernel_usage_percent.resize(logical, 0.0);
        Self {
            cpu,
            prev_ticks: Vec::new(),
            static_loaded: false,
        }
    }

    fn refresh(&mut self) {
        if !self.static_loaded {
            self.load_static();
        }
        self.refresh_dynamic();
    }

    fn cached(&self) -> &Cpu {
        &self.cpu
    }
}

fn read_per_cpu_ticks(cpu_count: usize) -> Vec<CpuTicks> {
    unsafe {
        use mach2::kern_return::KERN_SUCCESS;
        use mach2::mach_types::host_t;
        use mach2::message::mach_msg_type_number_t;
        use mach2::vm_types::natural_t;

        extern "C" {
            fn mach_host_self() -> host_t;
            fn host_processor_info(
                host: host_t,
                flavor: i32,
                out_processor_count: *mut natural_t,
                out_processor_info: *mut *mut i32,
                out_processor_info_count: *mut mach_msg_type_number_t,
            ) -> i32;
            fn vm_deallocate(target_task: u32, address: usize, size: usize) -> i32;
            static mach_task_self_: u32;
        }

        const PROCESSOR_CPU_LOAD_INFO: i32 = 2;
        const CPU_STATE_USER: usize = 0;
        const CPU_STATE_SYSTEM: usize = 1;
        const CPU_STATE_IDLE: usize = 2;
        const CPU_STATE_NICE: usize = 3;
        const CPU_STATE_MAX: usize = 4;

        let mut cpu_count_out: natural_t = 0;
        let mut cpu_info_ptr: *mut i32 = std::ptr::null_mut();
        let mut cpu_info_count: mach_msg_type_number_t = 0;

        let kr = host_processor_info(
            mach_host_self(),
            PROCESSOR_CPU_LOAD_INFO,
            &mut cpu_count_out,
            &mut cpu_info_ptr,
            &mut cpu_info_count,
        );

        if kr != KERN_SUCCESS || cpu_info_ptr.is_null() {
            return (0..cpu_count)
                .map(|_| CpuTicks {
                    user: 0,
                    system: 0,
                    idle: 0,
                    nice: 0,
                })
                .collect();
        }

        let slice = std::slice::from_raw_parts(cpu_info_ptr, cpu_info_count as usize);
        let mut ticks = Vec::with_capacity(cpu_count_out as usize);

        for i in 0..cpu_count_out as usize {
            let base = i * CPU_STATE_MAX;
            if base + CPU_STATE_NICE >= slice.len() {
                break;
            }
            ticks.push(CpuTicks {
                user: slice[base + CPU_STATE_USER] as u64,
                system: slice[base + CPU_STATE_SYSTEM] as u64,
                idle: slice[base + CPU_STATE_IDLE] as u64,
                nice: slice[base + CPU_STATE_NICE] as u64,
            });
        }

        let _ = vm_deallocate(
            mach_task_self_,
            cpu_info_ptr as usize,
            cpu_info_count as usize * std::mem::size_of::<i32>(),
        );

        ticks
    }
}

fn process_and_thread_count() -> (u64, u64) {
    // Sum pti_threadnum across all PIDs (same source as processes page).
    unsafe {
        let count = libc::proc_listallpids(std::ptr::null_mut(), 0);
        if count <= 0 {
            return (0, 0);
        }
        let mut buf: Vec<libc::pid_t> = vec![0; count as usize + 64];
        let actual = libc::proc_listallpids(
            buf.as_mut_ptr() as *mut libc::c_void,
            (buf.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int,
        );
        if actual <= 0 {
            return (0, 0);
        }

        let mut procs = 0u64;
        let mut threads = 0u64;
        for &pid in &buf[..actual as usize] {
            if pid <= 0 {
                continue;
            }
            let mut info: libc::proc_taskinfo = std::mem::zeroed();
            let ret = libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTASKINFO,
                0,
                &mut info as *mut libc::proc_taskinfo as *mut libc::c_void,
                std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int,
            );
            if ret > 0 {
                procs += 1;
                threads += info.pti_threadnum.max(0) as u64;
            }
        }
        (procs, threads)
    }
}
