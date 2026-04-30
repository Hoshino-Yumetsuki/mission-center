#![allow(dead_code)]
/* sys_info_v2/gatherer/src/platform/macos/processes.rs
 *
 * Copyright 2024 Mission Center Contributors
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use crate::dbus_shim::{Append, Arg, ArgType, IterAppend, Signature};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crate::platform::processes::*;

use super::MIN_DELTA_REFRESH;

#[allow(dead_code)]
const STALE_DELTA: std::time::Duration = std::time::Duration::from_millis(1000);

#[derive(Debug, Copy, Clone)]
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

#[derive(Debug, Clone)]
pub struct MacosProcess {
    name: Arc<str>,
    cmd: Vec<Arc<str>>,
    exe: Arc<str>,
    state: ProcessState,
    pid: u32,
    parent: u32,
    pub usage_stats: ProcessUsageStats,
    task_count: usize,
    raw_stats: RawStats,
}

impl Default for MacosProcess {
    fn default() -> Self {
        Self {
            name: Arc::from(""),
            cmd: vec![],
            exe: Arc::from(""),
            state: ProcessState::Unknown,
            pid: 0,
            parent: 0,
            usage_stats: ProcessUsageStats::default(),
            task_count: 1,
            raw_stats: RawStats::default(),
        }
    }
}

impl<'a> ProcessExt<'a> for MacosProcess {
    type Iter = std::iter::Map<std::slice::Iter<'a, Arc<str>>, fn(&'a Arc<str>) -> &'a str>;

    fn name(&self) -> &str {
        self.name.as_ref()
    }
    fn cmd(&'a self) -> Self::Iter {
        self.cmd.iter().map(|s| s.as_ref())
    }
    fn exe(&self) -> &str {
        self.exe.as_ref()
    }
    fn state(&self) -> ProcessState {
        self.state
    }
    fn pid(&self) -> u32 {
        self.pid
    }
    fn parent(&self) -> u32 {
        self.parent
    }
    fn usage_stats(&self) -> &ProcessUsageStats {
        &self.usage_stats
    }
    fn task_count(&self) -> usize {
        self.task_count
    }
}

#[derive(Default)]
pub struct MacosProcesses {
    processes: HashMap<u32, MacosProcess>,
    last_refresh: Option<Instant>,
    total_memory_bytes: u64,
}

impl MacosProcesses {
    pub fn new() -> Self { Self::default() }
}

impl<'a> ProcessesExt<'a> for MacosProcesses {
    type P = MacosProcess;

    fn refresh_cache(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last_refresh {
            if now.duration_since(last) < MIN_DELTA_REFRESH {
                return;
            }
        }
        self.last_refresh = Some(now);
        self.total_memory_bytes = read_total_memory();
        self.refresh_processes(now);
    }

    fn process_list(&'a self) -> &'a HashMap<u32, Self::P> {
        &self.processes
    }

    fn process_list_mut(&'a mut self) -> &'a mut HashMap<u32, Self::P> {
        &mut self.processes
    }

    fn terminate_process(&self, pid: u32) {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }

    fn kill_process(&self, pid: u32) {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
    }
}

impl MacosProcesses {
    fn refresh_processes(&mut self, now: Instant) {
        let pids = list_pids();
        let _total_mem = self.total_memory_bytes.max(1);

        let mut new_map: HashMap<u32, MacosProcess> = HashMap::with_capacity(pids.len());

        for pid in pids {
            let info = match read_proc_task_info(pid) {
                Some(i) => i,
                None => continue,
            };

            let name = read_proc_name(pid);
            let exe = read_proc_exe(pid);
            let cmd = read_proc_cmdline(pid);
            let parent = read_proc_ppid(pid);

            let prev = self.processes.get(&pid);
            let prev_stats = prev.map(|p| p.raw_stats).unwrap_or_else(|| RawStats {
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
            // Emit per-core percentage (0–100 per core, can exceed 100 for multi-threaded).
            // The UI normalizes via max_cpu_usage = logical_cpu_count * 100.
            // Do NOT divide by CPU_COUNT here — that would double-normalize.
            let cpu_usage = (((user_delta + sys_delta) as f64
                / (elapsed_secs * 1_000_000.0)
                * 100.0) as f32)
                .min((*super::CPU_COUNT as f32) * 100.0);

            let mem_usage = info.resident_bytes as f32;

            let disk_read_delta = info
                .disk_read_bytes
                .saturating_sub(prev_stats.disk_read_bytes);
            let disk_write_delta = info
                .disk_write_bytes
                .saturating_sub(prev_stats.disk_write_bytes);
            let disk_usage =
                ((disk_read_delta + disk_write_delta) as f64 / elapsed_secs) as f32;

            let state = map_proc_state(info.status);

            new_map.insert(
                pid,
                MacosProcess {
                    name: Arc::from(name.as_str()),
                    cmd,
                    exe: Arc::from(exe.as_str()),
                    state,
                    pid,
                    parent,
                    usage_stats: ProcessUsageStats {
                        cpu_usage,
                        memory_usage: mem_usage,
                        disk_usage,
                        network_usage: 0.0,
                        gpu_usage: 0.0,
                        gpu_memory_usage: 0.0,
                    },
                    task_count: info.thread_count as usize,
                    raw_stats: RawStats {
                        user_time_us: info.user_time_us,
                        sys_time_us: info.sys_time_us,
                        disk_read_bytes: info.disk_read_bytes,
                        disk_write_bytes: info.disk_write_bytes,
                        timestamp: now,
                    },
                },
            );
        }

        self.processes = new_map;
    }
}

struct ProcTaskInfo {
    user_time_us: u64,
    sys_time_us: u64,
    resident_bytes: u64,
    disk_read_bytes: u64,
    disk_write_bytes: u64,
    thread_count: u32,
    status: u32,
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

        let user_time_us = info.pti_total_user / 1000;
        let sys_time_us = info.pti_total_system / 1000;
        let resident_bytes = info.pti_resident_size;

        let mut disk_info: libc::rusage_info_v2 = std::mem::zeroed();
        let disk_ret = libc::proc_pid_rusage(
            pid as libc::pid_t,
            libc::RUSAGE_INFO_V2,
            &mut disk_info as *mut libc::rusage_info_v2 as *mut libc::rusage_info_t,
        );
        let (disk_read_bytes, disk_write_bytes) = if disk_ret == 0 {
            (disk_info.ri_diskio_bytesread, disk_info.ri_diskio_byteswritten)
        } else {
            (0, 0)
        };

        Some(ProcTaskInfo {
            user_time_us,
            sys_time_us,
            resident_bytes,
            disk_read_bytes,
            disk_write_bytes,
            thread_count: info.pti_threadnum as u32,
            status: info.pti_numrunning as u32,
        })
    }
}

fn read_proc_name(pid: u32) -> String {
    unsafe {
        // proc_name requires a buffer larger than MAXCOMLEN+1 (17 bytes) to succeed.
        // Use 1024 bytes to be safe.
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

fn read_proc_cmdline(pid: u32) -> Vec<Arc<str>> {
    unsafe {
        let mib = [
            libc::CTL_KERN,
            libc::KERN_PROCARGS2,
            pid as libc::c_int,
        ];
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
                let s = String::from_utf8_lossy(&rest[pos..end]).into_owned();
                args.push(Arc::from(s.as_str()));
            }
            pos = end + 1;
        }
        args
    }
}

fn read_proc_ppid(pid: u32) -> u32 {
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
            0
        } else {
            info.pbi_ppid
        }
    }
}

fn map_proc_state(status: u32) -> ProcessState {
    match status {
        1 => ProcessState::Sleeping,
        2 => ProcessState::Running,
        3 => ProcessState::Stopped,
        4 => ProcessState::Zombie,
        _ => ProcessState::Unknown,
    }
}

fn read_total_memory() -> u64 {
    unsafe {
        let mut mem: u64 = 0;
        let mut size = std::mem::size_of::<u64>();
        libc::sysctlbyname(
            b"hw.memsize\0".as_ptr() as *const libc::c_char,
            &mut mem as *mut u64 as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        );
        mem
    }
}

impl Arg for MacosProcesses {
    const ARG_TYPE: ArgType = ArgType::Struct;
    fn signature() -> Signature { Signature::from("") }
}
impl Append for MacosProcesses {
    fn append_by_ref(&self, _: &mut IterAppend) {}
}
