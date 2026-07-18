/* src/processes.rs
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

use magpie_platform::processes::{NetworkStatsError, Process, ProcessState, ProcessUsageStats};

use crate::util;

#[derive(Debug, Clone, Copy)]
struct RawStats {
    user_time_us: u64,
    sys_time_us: u64,
    disk_read_bytes: u64,
    disk_write_bytes: u64,
    timestamp: Instant,
}

impl Default for RawStats {
    fn default() -> Self {
        Self {
            user_time_us: 0,
            sys_time_us: 0,
            disk_read_bytes: 0,
            disk_write_bytes: 0,
            timestamp: Instant::now(),
        }
    }
}

pub struct ProcessCache {
    processes: HashMap<u32, Process>,
    raw_stats: HashMap<u32, RawStats>,
    network_stats_error: Option<NetworkStatsError>,
    cpu_count: f32,
}

impl magpie_platform::processes::ProcessCache for ProcessCache {
    fn new() -> Self {
        let cpu_count = util::sysctl_u32("hw.logicalcpu")
            .or_else(|| util::sysctl_u32("hw.ncpu"))
            .unwrap_or(1)
            .max(1) as f32;

        Self {
            processes: HashMap::new(),
            raw_stats: HashMap::new(),
            network_stats_error: None,
            cpu_count,
        }
    }

    fn refresh(&mut self) {
        let now = Instant::now();
        let pids = list_pids();

        let mut new_map: HashMap<u32, Process> = HashMap::with_capacity(pids.len());
        let mut new_stats: HashMap<u32, RawStats> = HashMap::with_capacity(pids.len());
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();

        for pid in pids {
            let info = match read_proc_task_info(pid) {
                Some(i) => i,
                None => continue,
            };

            let name = read_proc_name(pid);
            let exe = read_proc_exe(pid);
            let cmd = read_proc_cmdline(pid);
            let (parent, state) = read_proc_bsdinfo(pid);

            let prev_stats = self.raw_stats.get(&pid).copied().unwrap_or(RawStats {
                user_time_us: info.user_time_us,
                sys_time_us: info.sys_time_us,
                disk_read_bytes: info.disk_read_bytes,
                disk_write_bytes: info.disk_write_bytes,
                timestamp: now,
            });

            let elapsed_secs = now
                .duration_since(prev_stats.timestamp)
                .as_secs_f64()
                .max(0.001);

            let user_delta = info.user_time_us.saturating_sub(prev_stats.user_time_us);
            let sys_delta = info.sys_time_us.saturating_sub(prev_stats.sys_time_us);
            // Per-core % (can exceed 100 for multi-threaded). UI normalizes by logical count.
            let cpu_usage = (((user_delta + sys_delta) as f64 / (elapsed_secs * 1_000_000.0) * 100.0)
                as f32)
                .min(self.cpu_count * 100.0);

            let disk_read_delta = info
                .disk_read_bytes
                .saturating_sub(prev_stats.disk_read_bytes);
            let disk_write_delta = info
                .disk_write_bytes
                .saturating_sub(prev_stats.disk_write_bytes);
            let disk_usage = ((disk_read_delta + disk_write_delta) as f64 / elapsed_secs) as f32;

            let mut process = self.processes.remove(&pid).unwrap_or_default();
            process.name = name;
            process.cmd = cmd;
            process.exe = exe;
            process.state = state as i32;
            process.pid = pid;
            process.parent = parent;
            process.task_count = info.thread_count as u64;
            process.children.clear();
            process.usage_stats = ProcessUsageStats {
                cpu_usage,
                memory_usage: info.resident_bytes,
                swap_usage: 0,
                disk_usage,
                network_usage: 0.0,
                gpu_usage: 0.0,
                gpu_memory_usage: 0,
            };

            children.entry(parent).or_default().push(pid);
            new_stats.insert(
                pid,
                RawStats {
                    user_time_us: info.user_time_us,
                    sys_time_us: info.sys_time_us,
                    disk_read_bytes: info.disk_read_bytes,
                    disk_write_bytes: info.disk_write_bytes,
                    timestamp: now,
                },
            );
            new_map.insert(pid, process);
        }

        children.remove(&0);
        for (parent, kids) in children {
            if let Some(parent) = new_map.get_mut(&parent) {
                parent.children = kids;
            }
        }

        self.processes = new_map;
        self.raw_stats = new_stats;
    }

    fn cached_entries(&self) -> &HashMap<u32, Process> {
        &self.processes
    }

    fn cached_network_stats_error(&self) -> &Option<NetworkStatsError> {
        &self.network_stats_error
    }
}

pub struct ProcessManager;

impl magpie_platform::processes::ProcessManager for ProcessManager {
    fn new() -> Self {
        Self
    }

    fn terminate_processes(&self, pids: Vec<u32>) {
        send_signals(&pids, libc::SIGTERM);
    }

    fn kill_processes(&self, pids: Vec<u32>) {
        send_signals(&pids, libc::SIGKILL);
    }

    fn interrupt_processes(&self, pids: Vec<u32>) {
        send_signals(&pids, libc::SIGINT);
    }

    fn signal_user_one_processes(&self, pids: Vec<u32>) {
        send_signals(&pids, libc::SIGUSR1);
    }

    fn signal_user_two_processes(&self, pids: Vec<u32>) {
        send_signals(&pids, libc::SIGUSR2);
    }

    fn hangup_processes(&self, pids: Vec<u32>) {
        send_signals(&pids, libc::SIGHUP);
    }

    fn continue_processes(&self, pids: Vec<u32>) {
        send_signals(&pids, libc::SIGCONT);
    }

    fn suspend_processes(&self, pids: Vec<u32>) {
        send_signals(&pids, libc::SIGSTOP);
    }
}

fn send_signals(pids: &[u32], signal: libc::c_int) {
    for pid in pids {
        let rc = unsafe { libc::kill(*pid as libc::pid_t, signal) };
        if rc != 0 {
            log::debug!(
                "kill({}, {}) failed: {}",
                pid,
                signal,
                std::io::Error::last_os_error()
            );
        }
    }
}

struct ProcTaskInfo {
    user_time_us: u64,
    sys_time_us: u64,
    resident_bytes: u64,
    disk_read_bytes: u64,
    disk_write_bytes: u64,
    thread_count: u32,
}

fn list_pids() -> Vec<u32> {
    unsafe {
        let count = libc::proc_listallpids(std::ptr::null_mut(), 0);
        if count <= 0 {
            return vec![];
        }
        let mut buf: Vec<libc::pid_t> = vec![0; count as usize + 64];
        let actual = libc::proc_listallpids(
            buf.as_mut_ptr() as *mut libc::c_void,
            (buf.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int,
        );
        if actual <= 0 {
            return vec![];
        }
        buf[..actual as usize]
            .iter()
            .filter(|&&p| p > 0)
            .map(|&p| p as u32)
            .collect()
    }
}

fn read_proc_task_info(pid: u32) -> Option<ProcTaskInfo> {
    unsafe {
        let mut info: libc::proc_taskinfo = std::mem::zeroed();
        let ret = libc::proc_pidinfo(
            pid as libc::pid_t,
            libc::PROC_PIDTASKINFO,
            0,
            &mut info as *mut libc::proc_taskinfo as *mut libc::c_void,
            std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int,
        );
        if ret <= 0 {
            return None;
        }

        // pti_total_{user,system} are nanoseconds on modern macOS.
        let user_time_us = info.pti_total_user / 1000;
        let sys_time_us = info.pti_total_system / 1000;

        let mut disk_info: libc::rusage_info_v2 = std::mem::zeroed();
        let disk_ret = libc::proc_pid_rusage(
            pid as libc::pid_t,
            libc::RUSAGE_INFO_V2,
            &mut disk_info as *mut libc::rusage_info_v2 as *mut libc::rusage_info_t,
        );
        let (disk_read_bytes, disk_write_bytes) = if disk_ret == 0 {
            (
                disk_info.ri_diskio_bytesread,
                disk_info.ri_diskio_byteswritten,
            )
        } else {
            (0, 0)
        };

        Some(ProcTaskInfo {
            user_time_us,
            sys_time_us,
            resident_bytes: info.pti_resident_size,
            disk_read_bytes,
            disk_write_bytes,
            thread_count: info.pti_threadnum as u32,
        })
    }
}

fn read_proc_name(pid: u32) -> String {
    unsafe {
        let mut buf = [0u8; 1024];
        let ret = libc::proc_name(
            pid as libc::pid_t,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len() as u32,
        );
        if ret <= 0 {
            return format!("pid:{}", pid);
        }
        let len = buf.iter().position(|&b| b == 0).unwrap_or(ret as usize);
        String::from_utf8_lossy(&buf[..len]).into_owned()
    }
}

fn read_proc_exe(pid: u32) -> String {
    unsafe {
        let mut buf = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        let ret = libc::proc_pidpath(
            pid as libc::pid_t,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len() as u32,
        );
        if ret <= 0 {
            return String::new();
        }
        let len = buf.iter().position(|&b| b == 0).unwrap_or(ret as usize);
        String::from_utf8_lossy(&buf[..len]).into_owned()
    }
}

fn read_proc_cmdline(pid: u32) -> Vec<String> {
    unsafe {
        let mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];
        let mut size: libc::size_t = 0;
        if libc::sysctl(
            mib.as_ptr() as *mut libc::c_int,
            3,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return vec![];
        }
        let mut buf: Vec<u8> = vec![0u8; size];
        if libc::sysctl(
            mib.as_ptr() as *mut libc::c_int,
            3,
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return vec![];
        }
        if size < 4 {
            return vec![];
        }
        let argc = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        let rest = &buf[4..size];
        let exe_end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        let args_start = rest[exe_end..]
            .iter()
            .position(|&b| b != 0)
            .map(|p| exe_end + p)
            .unwrap_or(rest.len());

        let mut args = Vec::with_capacity(argc);
        let mut pos = args_start;
        while pos < rest.len() && args.len() < argc {
            let end = rest[pos..]
                .iter()
                .position(|&b| b == 0)
                .map(|p| pos + p)
                .unwrap_or(rest.len());
            if end > pos {
                args.push(String::from_utf8_lossy(&rest[pos..end]).into_owned());
            }
            pos = end + 1;
        }
        args
    }
}

fn read_proc_bsdinfo(pid: u32) -> (u32, ProcessState) {
    unsafe {
        let mut info: libc::proc_bsdinfo = std::mem::zeroed();
        let ret = libc::proc_pidinfo(
            pid as libc::pid_t,
            libc::PROC_PIDTBSDINFO,
            0,
            &mut info as *mut libc::proc_bsdinfo as *mut libc::c_void,
            std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int,
        );
        if ret <= 0 {
            return (0, ProcessState::Unknown);
        }
        (info.pbi_ppid, map_proc_state(info.pbi_status))
    }
}

// BSD process states from sys/proc.h
fn map_proc_state(status: u32) -> ProcessState {
    match status {
        1 => ProcessState::Unknown, // SIDL
        2 => ProcessState::Running, // SRUN
        3 => ProcessState::Sleeping, // SSLEEP
        4 => ProcessState::Stopped, // SSTOP
        5 => ProcessState::Zombie,  // SZOMB
        _ => ProcessState::Unknown,
    }
}
