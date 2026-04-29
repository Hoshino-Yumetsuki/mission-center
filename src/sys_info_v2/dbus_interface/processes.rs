/* sys_info_v2/dbus_interface/processes.rs
 *
 * Copyright 2024 Romeo Calota
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

use std::{collections::HashMap, sync::Arc};

#[cfg(target_os = "linux")]
use dbus::{arg::*, strings::*};

#[derive(Debug, Copy, Clone)]
#[repr(u8)]
pub enum ProcessState {
    Running = 0,
    Sleeping = 1,
    SleepingUninterruptible = 2,
    Zombie = 3,
    Stopped = 4,
    Tracing = 5,
    Dead = 6,
    WakeKill = 7,
    Waking = 8,
    Parked = 9,
    Unknown = 10,
}

#[derive(Debug, Default, Copy, Clone)]
pub struct ProcessUsageStats {
    pub cpu_usage: f32,
    pub memory_usage: f32,
    pub disk_usage: f32,
    pub network_usage: f32,
    pub gpu_usage: f32,
    pub gpu_memory_usage: f32,
}

impl ProcessUsageStats {
    pub fn merge(&mut self, other: &Self) {
        self.cpu_usage += other.cpu_usage;
        self.memory_usage += other.memory_usage;
        self.disk_usage += other.disk_usage;
        self.network_usage += other.network_usage;
        self.gpu_usage += other.gpu_usage;
        self.gpu_memory_usage += other.gpu_memory_usage;
    }
}

#[derive(Debug, Clone)]
pub struct Process {
    pub name: Arc<str>,
    pub cmd: Vec<Arc<str>>,
    pub exe: Arc<str>,
    pub state: ProcessState,
    pub pid: u32,
    pub parent: u32,
    pub usage_stats: ProcessUsageStats,
    pub merged_usage_stats: ProcessUsageStats,
    pub task_count: usize,
    pub children: HashMap<u32, Process>,
}

impl Default for Process {
    fn default() -> Self {
        let empty_string = Arc::<str>::from("");

        Self {
            name: empty_string.clone(),
            cmd: vec![],
            exe: empty_string,
            state: ProcessState::Unknown,
            pid: 0,
            parent: 0,
            usage_stats: Default::default(),
            merged_usage_stats: Default::default(),
            task_count: 0,
            children: HashMap::new(),
        }
    }
}

#[cfg(target_os = "linux")]
impl From<&dyn RefArg> for Process {
    fn from(value: &dyn RefArg) -> Self {
        use gtk::glib::g_critical;

        let mut this = Self::default();

        let mut process = match value.as_iter() {
            None => {
                g_critical!(
                    "MissionCenter::GathererDBusProxy",
                    "Failed to get Process: Expected '0: STRUCT', got None, failed to iterate over fields",
                );
                return this;
            }
            Some(i) => i,
        };
        let process = process.as_mut();

        this.name = match Iterator::next(process) {
            None => { g_critical!("MissionCenter::GathererDBusProxy", "Failed to get Process: Expected '0: s', got None"); return this; }
            Some(arg) => match arg.as_str() {
                None => { g_critical!("MissionCenter::GathererDBusProxy", "Failed to get Process: Expected '0: s', got {:?}", arg.arg_type()); return this; }
                Some(n) => Arc::from(n),
            },
        };

        match Iterator::next(process) {
            None => { g_critical!("MissionCenter::GathererDBusProxy", "Failed to get Process: Expected '1: ARRAY', got None"); return this; }
            Some(arg) => match arg.as_iter() {
                None => { g_critical!("MissionCenter::GathererDBusProxy", "Failed to get Process: Expected '1: ARRAY', got {:?}", arg.arg_type()); return this; }
                Some(cmds) => {
                    for c in cmds {
                        if let Some(c) = c.as_str() {
                            this.cmd.push(Arc::from(c));
                        }
                    }
                }
            },
        }

        this.exe = match Iterator::next(process) {
            None => { g_critical!("MissionCenter::GathererDBusProxy", "Failed to get Process: Expected '3: s', got None"); return this; }
            Some(arg) => match arg.as_str() {
                None => { g_critical!("MissionCenter::GathererDBusProxy", "Failed to get Process: Expected '3: s', got {:?}", arg.arg_type()); return this; }
                Some(e) => Arc::from(e),
            },
        };

        this.state = match Iterator::next(process) {
            None => { g_critical!("MissionCenter::GathererDBusProxy", "Failed to get Process: Expected '4: y', got None"); return this; }
            Some(arg) => match arg.as_u64() {
                None => { g_critical!("MissionCenter::GathererDBusProxy", "Failed to get Process: Expected '4: y', got {:?}", arg.arg_type()); return this; }
                Some(u) => {
                    if u < ProcessState::Unknown as u64 {
                        unsafe { core::mem::transmute(u as u8) }
                    } else {
                        ProcessState::Unknown
                    }
                }
            },
        };

        this.pid = match Iterator::next(process) {
            None => { g_critical!("MissionCenter::GathererDBusProxy", "Failed to get Process: Expected '5: u', got None"); return this; }
            Some(arg) => match arg.as_u64() {
                None => { g_critical!("MissionCenter::GathererDBusProxy", "Failed to get Process: Expected '5: u', got {:?}", arg.arg_type()); return this; }
                Some(p) => p as _,
            },
        };

        this.parent = match Iterator::next(process) {
            None => { g_critical!("MissionCenter::GathererDBusProxy", "Failed to get Process: Expected '6: u', got None"); return this; }
            Some(arg) => match arg.as_u64() {
                None => { g_critical!("MissionCenter::GathererDBusProxy", "Failed to get Process: Expected '6: u', got {:?}", arg.arg_type()); return this; }
                Some(p) => p as _,
            },
        };

        match Iterator::next(process) {
            None => { g_critical!("MissionCenter::GathererDBusProxy", "Failed to get Process: Expected '7: STRUCT', got None"); return this; }
            Some(arg) => match arg.as_iter() {
                None => { g_critical!("MissionCenter::GathererDBusProxy", "Failed to get Process: Expected '7: STRUCT', got {:?}", arg.arg_type()); return this; }
                Some(stats) => {
                    let mut values = [0_f32; 6];
                    for (i, v) in stats.enumerate() {
                        values[i] = v.as_f64().unwrap_or(0.) as f32;
                    }
                    this.usage_stats.cpu_usage = values[0];
                    this.usage_stats.memory_usage = values[1];
                    this.usage_stats.disk_usage = values[2];
                    this.usage_stats.network_usage = values[3];
                    this.usage_stats.gpu_usage = values[4];
                    this.usage_stats.gpu_memory_usage = values[5];
                    this.merged_usage_stats = this.usage_stats;
                }
            },
        };

        this.task_count = match Iterator::next(process) {
            None => { g_critical!("MissionCenter::GathererDBusProxy", "Failed to get Process: Expected '14: t', got None"); return this; }
            Some(arg) => match arg.as_u64() {
                None => { g_critical!("MissionCenter::GathererDBusProxy", "Failed to get Process: Expected '14: t', got {:?}", arg.arg_type()); return this; }
                Some(tc) => tc as _,
            },
        };

        this
    }
}

pub struct ProcessMap(HashMap<u32, Process>);

impl From<HashMap<u32, Process>> for ProcessMap {
    fn from(value: HashMap<u32, Process>) -> Self {
        Self(value)
    }
}

impl From<ProcessMap> for HashMap<u32, Process> {
    fn from(value: ProcessMap) -> Self {
        value.0
    }
}

#[cfg(target_os = "linux")]
impl Arg for ProcessMap {
    const ARG_TYPE: ArgType = ArgType::Array;

    fn signature() -> Signature<'static> {
        Signature::from("a(sassyuu(dddddd)t)")
    }
}

#[cfg(target_os = "linux")]
impl ReadAll for ProcessMap {
    fn read(i: &mut Iter) -> Result<Self, TypeMismatchError> {
        i.get().ok_or(super::TypeMismatchError::new(
            ArgType::Invalid,
            ArgType::Invalid,
            0,
        ))
    }
}

#[cfg(target_os = "linux")]
impl<'a> Get<'a> for ProcessMap {
    fn get(i: &mut Iter<'a>) -> Option<Self> {
        use gtk::glib::g_critical;

        let mut this = HashMap::new();

        match Iterator::next(i) {
            None => {
                g_critical!(
                    "MissionCenter::GathererDBusProxy",
                    "Failed to get HashMap<Pid, Process>: Expected '0: ARRAY', got None",
                );
                return None;
            }
            Some(arg) => match arg.as_iter() {
                None => {
                    g_critical!(
                        "MissionCenter::GathererDBusProxy",
                        "Failed to get HashMap<Pid, Process>: Expected '0: ARRAY', got {:?}",
                        arg.arg_type(),
                    );
                    return None;
                }
                Some(arr) => {
                    for p in arr {
                        let p = Process::from(p);
                        if p.pid == 0 {
                            continue;
                        }
                        this.insert(p.pid, p.clone());
                    }
                }
            },
        }

        Some(this.into())
    }
}

#[cfg(target_os = "macos")]
impl<'de> serde::Deserialize<'de> for ProcessMap {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let arr = Vec::<serde_json::Value>::deserialize(deserializer)?;
        let mut map = HashMap::new();
        for v in arr {
            let mut p = Process::default();
            if let Some(s) = v.get("name").and_then(|x| x.as_str()) { p.name = Arc::from(s); }
            if let Some(a) = v.get("cmd").and_then(|x| x.as_array()) {
                p.cmd = a.iter().filter_map(|x| x.as_str()).map(Arc::from).collect();
            }
            if let Some(s) = v.get("exe").and_then(|x| x.as_str()) { p.exe = Arc::from(s); }
            if let Some(n) = v.get("pid").and_then(|x| x.as_u64()) { p.pid = n as u32; }
            if let Some(n) = v.get("parent").and_then(|x| x.as_u64()) { p.parent = n as u32; }
            if let Some(n) = v.get("task_count").and_then(|x| x.as_u64()) { p.task_count = n as usize; }
            if let Some(stats) = v.get("usage_stats") {
                if let Some(f) = stats.get("cpu_usage").and_then(|x| x.as_f64()) { p.usage_stats.cpu_usage = f as f32; }
                if let Some(f) = stats.get("memory_usage").and_then(|x| x.as_f64()) { p.usage_stats.memory_usage = f as f32; }
                if let Some(f) = stats.get("disk_usage").and_then(|x| x.as_f64()) { p.usage_stats.disk_usage = f as f32; }
                if let Some(f) = stats.get("network_usage").and_then(|x| x.as_f64()) { p.usage_stats.network_usage = f as f32; }
                if let Some(f) = stats.get("gpu_usage").and_then(|x| x.as_f64()) { p.usage_stats.gpu_usage = f as f32; }
                if let Some(f) = stats.get("gpu_memory_usage").and_then(|x| x.as_f64()) { p.usage_stats.gpu_memory_usage = f as f32; }
                p.merged_usage_stats = p.usage_stats;
            }
            if p.pid == 0 { continue; }
            map.insert(p.pid, p);
        }
        Ok(ProcessMap(map))
    }
}
