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

#[cfg(not(target_arch = "aarch64"))]
use crate::fan::smc_cpu_temperature;
#[cfg(target_arch = "aarch64")]
use crate::sensors::apple_silicon_temperature;
use crate::util::{sysctl_string, sysctl_u32, sysctl_u64, uptime_seconds};

#[derive(Clone, Copy)]
struct CpuTicks {
    user: u32,
    system: u32,
    idle: u32,
    nice: u32,
}

impl CpuTicks {
    fn usage_since(self, prev: Self) -> (f32, f32) {
        let user = u64::from(self.user.wrapping_sub(prev.user));
        let system = u64::from(self.system.wrapping_sub(prev.system));
        let idle = u64::from(self.idle.wrapping_sub(prev.idle));
        let nice = u64::from(self.nice.wrapping_sub(prev.nice));
        let total = (user + system + idle + nice).max(1) as f32;
        (
            (user + system + nice) as f32 / total * 100.0,
            system as f32 / total * 100.0,
        )
    }
}

pub struct CpuCache {
    cpu: Cpu,
    prev_ticks: Vec<CpuTicks>,
    static_loaded: bool,
}

impl CpuCache {
    fn load_static(&mut self) {
        let logical = sysctl_u32("hw.logicalcpu").filter(|&n| n > 0).unwrap_or(1) as usize;

        let info = &mut self.cpu;
        info.name = sysctl_string("machdep.cpu.brand_string").filter(|s| !s.is_empty());
        info.socket_count = sysctl_u32("hw.packages").filter(|&n| n > 0);
        info.base_freq_khz = sysctl_u64("hw.cpufrequency")
            .map(|f| f / 1000)
            .filter(|&f| f > 0);

        // Generic hw.l*cache* values describe one cache, not system totals.
        #[cfg(target_arch = "aarch64")]
        let (l1, l2) = apple_cache_totals();
        #[cfg(not(target_arch = "aarch64"))]
        let (l1, l2) = (None, None);
        info.l1_combined_cache_bytes = l1;
        info.l2_cache_bytes = l2;
        info.l3_cache_bytes = None;
        info.l4_cache_bytes = None;

        let is_vm = sysctl_u32("kern.hv_vmm_present").map(|v| v != 0);
        info.is_virtual_machine = is_vm;
        info.virtualization_technology = sysctl_u32("kern.hv_support")
            .filter(|&supported| supported == 1)
            .map(|_| "Hypervisor Framework".to_string());

        info.core_usage_percent.resize(logical, f32::NAN);
        info.core_kernel_usage_percent.resize(logical, f32::NAN);
        self.prev_ticks.clear();
        self.static_loaded = true;
    }

    fn refresh_dynamic(&mut self) {
        let ticks = read_per_cpu_ticks().unwrap_or_default();
        if !ticks.is_empty() && ticks.len() == self.prev_ticks.len() {
            for (i, (&prev, &curr)) in self.prev_ticks.iter().zip(&ticks).enumerate() {
                (
                    self.cpu.core_usage_percent[i],
                    self.cpu.core_kernel_usage_percent[i],
                ) = curr.usage_since(prev);
            }
            self.cpu.total_usage_percent =
                self.cpu.core_usage_percent.iter().sum::<f32>() / ticks.len() as f32;
            self.cpu.kernel_usage_percent =
                self.cpu.core_kernel_usage_percent.iter().sum::<f32>() / ticks.len() as f32;
        } else {
            // A rate needs two samples. Missing ticks are not an idle CPU.
            let n = if ticks.is_empty() {
                self.cpu.core_usage_percent.len()
            } else {
                ticks.len()
            };
            self.cpu.core_usage_percent = vec![f32::NAN; n];
            self.cpu.core_kernel_usage_percent = vec![f32::NAN; n];
            self.cpu.total_usage_percent = f32::NAN;
            self.cpu.kernel_usage_percent = f32::NAN;
        }
        self.prev_ticks = ticks;

        // No supported public macOS API reports the current CPU clock.
        self.cpu.current_frequency_mhz = 0;

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

#[cfg(target_arch = "aarch64")]
fn apple_cache_totals() -> (Option<u64>, Option<u64>) {
    fn collect() -> Option<(u64, u64)> {
        let levels = sysctl_u32("hw.nperflevels").filter(|&n| n > 0 && n <= 16)?;
        let mut l1 = 0u64;
        let mut l2 = 0u64;
        let mut physical_total = 0u32;
        for level in 0..levels {
            let prefix = format!("hw.perflevel{level}");
            let cores = sysctl_u32(&format!("{prefix}.physicalcpu"))?;
            let per_l2 = sysctl_u32(&format!("{prefix}.cpusperl2"))?;
            if cores == 0 || per_l2 == 0 || cores % per_l2 != 0 {
                return None;
            }
            physical_total = physical_total.checked_add(cores)?;
            let l1i = sysctl_u64(&format!("{prefix}.l1icachesize"))?;
            let l1d = sysctl_u64(&format!("{prefix}.l1dcachesize"))?;
            let l2_size = sysctl_u64(&format!("{prefix}.l2cachesize"))?;
            if l1i == 0 || l1d == 0 || l2_size == 0 {
                return None;
            }
            l1 = l1.checked_add(l1i.checked_add(l1d)?.checked_mul(cores.into())?)?;
            l2 = l2.checked_add(l2_size.checked_mul((cores / per_l2).into())?)?;
        }
        if sysctl_u32("hw.physicalcpu")? != physical_total {
            return None;
        }
        Some((l1, l2))
    }
    collect().map_or((None, None), |(l1, l2)| (Some(l1), Some(l2)))
}

impl magpie_platform::cpu::CpuCache for CpuCache {
    fn new() -> Self {
        let logical = sysctl_u32("hw.logicalcpu").unwrap_or(1) as usize;
        let mut cpu = Cpu {
            total_usage_percent: f32::NAN,
            kernel_usage_percent: f32::NAN,
            ..Default::default()
        };
        cpu.core_usage_percent.resize(logical, f32::NAN);
        cpu.core_kernel_usage_percent.resize(logical, f32::NAN);
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

fn read_per_cpu_ticks() -> Option<Vec<CpuTicks>> {
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

        let host = mach_host_self();
        let kr = host_processor_info(
            host,
            PROCESSOR_CPU_LOAD_INFO,
            &mut cpu_count_out,
            &mut cpu_info_ptr,
            &mut cpu_info_count,
        );
        let _ = mach2::mach_port::mach_port_deallocate(mach_task_self_, host);

        if kr != KERN_SUCCESS || cpu_info_ptr.is_null() {
            return None;
        }

        let slice = std::slice::from_raw_parts(cpu_info_ptr, cpu_info_count as usize);
        let mut ticks = Vec::with_capacity(cpu_count_out as usize);

        for i in 0..cpu_count_out as usize {
            let base = i * CPU_STATE_MAX;
            if base + CPU_STATE_NICE >= slice.len() {
                break;
            }
            ticks.push(CpuTicks {
                user: slice[base + CPU_STATE_USER] as u32,
                system: slice[base + CPU_STATE_SYSTEM] as u32,
                idle: slice[base + CPU_STATE_IDLE] as u32,
                nice: slice[base + CPU_STATE_NICE] as u32,
            });
        }

        let _ = vm_deallocate(
            mach_task_self_,
            cpu_info_ptr as usize,
            cpu_info_count as usize * std::mem::size_of::<i32>(),
        );

        if ticks.is_empty() || ticks.len() != cpu_count_out as usize {
            None
        } else {
            Some(ticks)
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use magpie_platform::cpu::CpuCache as _;

    #[test]
    fn tick_usage_includes_nice_and_handles_counter_wrap() {
        let prev = CpuTicks {
            user: u32::MAX - 4,
            system: 10,
            idle: 100,
            nice: 10,
        };
        let curr = CpuTicks {
            user: 5,
            system: 30,
            idle: 140,
            nice: 40,
        };
        let (total, kernel) = curr.usage_since(prev);
        assert!((total - 60.0).abs() < 0.001);
        assert!((kernel - 20.0).abs() < 0.001);
    }

    #[test]
    fn native_cpu_readings() {
        let mut cache = CpuCache::new();
        cache.refresh();
        std::thread::sleep(std::time::Duration::from_millis(300));
        cache.refresh();
        let cpu = cache.cached();
        if sysctl_u32("kern.hv_support") == Some(1) {
            assert_eq!(
                cpu.virtualization_technology.as_deref(),
                Some("Hypervisor Framework")
            );
        }
        assert!(!cpu.core_usage_percent.is_empty());
        assert_eq!(
            cpu.core_usage_percent.len(),
            read_per_cpu_ticks()
                .expect("Mach CPU ticks unavailable")
                .len()
        );
        assert_eq!(
            cpu.core_usage_percent.len(),
            cpu.core_kernel_usage_percent.len()
        );
        assert!(cpu
            .core_usage_percent
            .iter()
            .all(|&value| (0.0..=100.0).contains(&value)));
        assert!((0.0..=100.0).contains(&cpu.total_usage_percent));
        assert!(cpu.total_process_count > 0);
        assert!(cpu.uptime_seconds > 0);
        eprintln!("native CPU: {cpu:?}");
    }
}
