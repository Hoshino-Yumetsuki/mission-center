/* sys_info_v2/dbus_interface/mod.rs
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

use std::{
    collections::HashMap,
    num::NonZeroU32,
    sync::Arc,
};

#[cfg(target_os = "linux")]
use std::{
    mem::{align_of, size_of},
    rc::Rc,
};

#[cfg(target_os = "linux")]
use dbus::{
    arg::ArgType,
    blocking::{LocalConnection, Proxy},
};
#[cfg(target_os = "linux")]
use static_assertions::const_assert;

pub use apps::{App, AppMap};
pub use arc_str_vec::ArcStrVec;
pub use cpu_dynamic_info::CpuDynamicInfo;
pub use cpu_static_info::CpuStaticInfo;
pub use disk_info::{DiskInfo, DiskInfoVec, DiskType};
pub use fan_info::{FanInfo, FanInfoVec};
pub use gpu_dynamic_info::{GpuDynamicInfo, GpuDynamicInfoVec};
pub use gpu_static_info::{GpuStaticInfo, GpuStaticInfoVec, OpenGLApi};
pub use processes::{Process, ProcessMap, ProcessUsageStats};
pub use service::{Service, ServiceMap};

mod apps;
mod arc_str_vec;
mod cpu_dynamic_info;
mod cpu_static_info;
mod disk_info;
mod fan_info;
mod gpu_dynamic_info;
mod gpu_static_info;
mod processes;
mod service;

#[allow(dead_code)]
pub const MC_GATHERER_OBJECT_PATH: &str = "/io/missioncenter/MissionCenter/Gatherer";
#[allow(dead_code)]
pub const MC_GATHERER_INTERFACE_NAME: &str = "io.missioncenter.MissionCenter.Gatherer";

#[cfg(target_os = "linux")]
pub type GathererError = dbus::Error;

#[cfg(target_os = "macos")]
#[derive(Debug)]
pub struct GathererError(pub String);

#[cfg(target_os = "macos")]
impl std::fmt::Display for GathererError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(target_os = "linux")]
#[allow(unused)]
struct TypeMismatchError {
    pub expected: ArgType,
    pub found: ArgType,
    pub position: u32,
}

#[cfg(target_os = "linux")]
impl TypeMismatchError {
    pub fn new(expected: ArgType, found: ArgType, position: u32) -> dbus::arg::TypeMismatchError {
        unsafe {
            std::mem::transmute(Self {
                expected,
                found,
                position,
            })
        }
    }
}

#[cfg(target_os = "linux")]
const_assert!(size_of::<TypeMismatchError>() == size_of::<dbus::arg::TypeMismatchError>());
#[cfg(target_os = "linux")]
const_assert!(align_of::<TypeMismatchError>() == align_of::<dbus::arg::TypeMismatchError>());

#[allow(dead_code)]
pub trait Gatherer {
    fn get_cpu_static_info(&self) -> Result<CpuStaticInfo, GathererError>;
    fn get_cpu_dynamic_info(&self) -> Result<CpuDynamicInfo, GathererError>;
    fn get_disks_info(&self) -> Result<Vec<DiskInfo>, GathererError>;
    fn get_fans_info(&self) -> Result<Vec<FanInfo>, GathererError>;
    fn get_gpu_list(&self) -> Result<Vec<Arc<str>>, GathererError>;
    fn get_gpu_static_info(&self) -> Result<Vec<GpuStaticInfo>, GathererError>;
    fn get_gpu_dynamic_info(&self) -> Result<Vec<GpuDynamicInfo>, GathererError>;
    fn get_apps(&self) -> Result<HashMap<Arc<str>, App>, GathererError>;
    fn get_processes(&self) -> Result<HashMap<u32, Process>, GathererError>;
    fn get_services(&self) -> Result<HashMap<Arc<str>, Service>, GathererError>;
    fn terminate_process(&self, process_id: u32) -> Result<(), GathererError>;
    fn kill_process(&self, process_id: u32) -> Result<(), GathererError>;
    fn enable_service(&self, service_name: &str) -> Result<(), GathererError>;
    fn disable_service(&self, service_name: &str) -> Result<(), GathererError>;
    fn start_service(&self, service_name: &str) -> Result<(), GathererError>;
    fn stop_service(&self, service_name: &str) -> Result<(), GathererError>;
    fn restart_service(&self, service_name: &str) -> Result<(), GathererError>;
    fn get_service_logs(
        &self,
        service_name: &str,
        pid: Option<NonZeroU32>,
    ) -> Result<Arc<str>, GathererError>;
}

#[cfg(target_os = "linux")]
impl<'a> Gatherer for Proxy<'a, Rc<LocalConnection>> {
    fn get_cpu_static_info(&self) -> Result<CpuStaticInfo, GathererError> {
        self.method_call(MC_GATHERER_INTERFACE_NAME, "GetCPUStaticInfo", ())
    }

    fn get_cpu_dynamic_info(&self) -> Result<CpuDynamicInfo, GathererError> {
        self.method_call(MC_GATHERER_INTERFACE_NAME, "GetCPUDynamicInfo", ())
    }

    fn get_disks_info(&self) -> Result<Vec<DiskInfo>, GathererError> {
        let res: Result<DiskInfoVec, _> =
            self.method_call(MC_GATHERER_INTERFACE_NAME, "GetDisksInfo", ());
        res.map(|v| v.into())
    }

    fn get_fans_info(&self) -> Result<Vec<FanInfo>, GathererError> {
        let res: Result<FanInfoVec, _> =
            self.method_call(MC_GATHERER_INTERFACE_NAME, "GetFansInfo", ());
        res.map(|v| v.into())
    }

    fn get_gpu_list(&self) -> Result<Vec<Arc<str>>, GathererError> {
        let res: Result<ArcStrVec, _> =
            self.method_call(MC_GATHERER_INTERFACE_NAME, "GetGPUList", ());
        res.map(|v| v.into())
    }

    fn get_gpu_static_info(&self) -> Result<Vec<GpuStaticInfo>, GathererError> {
        let res: Result<GpuStaticInfoVec, _> =
            self.method_call(MC_GATHERER_INTERFACE_NAME, "GetGPUStaticInfo", ());
        res.map(|v| v.into())
    }

    fn get_gpu_dynamic_info(&self) -> Result<Vec<GpuDynamicInfo>, GathererError> {
        let res: Result<GpuDynamicInfoVec, _> =
            self.method_call(MC_GATHERER_INTERFACE_NAME, "GetGPUDynamicInfo", ());
        res.map(|v| v.into())
    }

    fn get_apps(&self) -> Result<HashMap<Arc<str>, App>, GathererError> {
        let res: Result<AppMap, _> = self.method_call(MC_GATHERER_INTERFACE_NAME, "GetApps", ());
        res.map(|v| v.into())
    }

    fn get_processes(&self) -> Result<HashMap<u32, Process>, GathererError> {
        let res: Result<ProcessMap, _> =
            self.method_call(MC_GATHERER_INTERFACE_NAME, "GetProcesses", ());
        res.map(|v| v.into())
    }

    fn get_services(&self) -> Result<HashMap<Arc<str>, Service>, GathererError> {
        let res: Result<ServiceMap, _> =
            self.method_call(MC_GATHERER_INTERFACE_NAME, "GetServices", ());
        res.map(|v| v.into())
    }

    fn terminate_process(&self, process_id: u32) -> Result<(), GathererError> {
        self.method_call(
            MC_GATHERER_INTERFACE_NAME,
            "TerminateProcess",
            (process_id,),
        )
    }

    fn kill_process(&self, process_id: u32) -> Result<(), GathererError> {
        self.method_call(MC_GATHERER_INTERFACE_NAME, "KillProcess", (process_id,))
    }

    fn enable_service(&self, service_name: &str) -> Result<(), GathererError> {
        self.method_call(MC_GATHERER_INTERFACE_NAME, "EnableService", (service_name,))
    }

    fn disable_service(&self, service_name: &str) -> Result<(), GathererError> {
        self.method_call(
            MC_GATHERER_INTERFACE_NAME,
            "DisableService",
            (service_name,),
        )
    }

    fn start_service(&self, service_name: &str) -> Result<(), GathererError> {
        self.method_call(MC_GATHERER_INTERFACE_NAME, "StartService", (service_name,))
    }

    fn stop_service(&self, service_name: &str) -> Result<(), GathererError> {
        self.method_call(MC_GATHERER_INTERFACE_NAME, "StopService", (service_name,))
    }

    fn restart_service(&self, service_name: &str) -> Result<(), GathererError> {
        self.method_call(
            MC_GATHERER_INTERFACE_NAME,
            "RestartService",
            (service_name,),
        )
    }

    fn get_service_logs(
        &self,
        service_name: &str,
        pid: Option<NonZeroU32>,
    ) -> Result<Arc<str>, GathererError> {
        let res: Result<(String,), _> = self.method_call(
            MC_GATHERER_INTERFACE_NAME,
            "GetServiceLogs",
            (service_name, pid.map(|v| v.get()).unwrap_or(0)),
        );
        res.map(|v| Arc::<str>::from(v.0))
    }
}

#[cfg(target_os = "macos")]
pub struct MacosGathererProxy {
    pub socket_path: String,
}

#[cfg(target_os = "macos")]
impl MacosGathererProxy {
    pub fn new(socket_path: String) -> Self {
        Self { socket_path }
    }

    pub(crate) fn call<T>(&self, method: &str, arg: Option<&str>) -> Result<T, GathererError>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixStream;

        let mut stream = UnixStream::connect(&self.socket_path)
            .map_err(|e| GathererError(format!("connect: {}", e)))?;

        let payload = match arg {
            Some(a) => format!("{}\0{}", method, a),
            None => format!("{}\0", method),
        };
        let len = payload.len() as u32;
        stream
            .write_all(&len.to_le_bytes())
            .map_err(|e| GathererError(format!("write len: {}", e)))?;
        stream
            .write_all(payload.as_bytes())
            .map_err(|e| GathererError(format!("write payload: {}", e)))?;

        let mut len_buf = [0u8; 4];
        stream
            .read_exact(&mut len_buf)
            .map_err(|e| GathererError(format!("read len: {}", e)))?;
        let resp_len = u32::from_le_bytes(len_buf) as usize;

        let mut resp_buf = vec![0u8; resp_len];
        stream
            .read_exact(&mut resp_buf)
            .map_err(|e| GathererError(format!("read response: {}", e)))?;

        serde_json::from_slice(&resp_buf)
            .map_err(|e| GathererError(format!("deserialize: {}", e)))
    }
}

#[cfg(target_os = "macos")]
impl Gatherer for MacosGathererProxy {
    fn get_cpu_static_info(&self) -> Result<CpuStaticInfo, GathererError> {
        self.call("GetCPUStaticInfo", None)
    }

    fn get_cpu_dynamic_info(&self) -> Result<CpuDynamicInfo, GathererError> {
        self.call("GetCPUDynamicInfo", None)
    }

    fn get_disks_info(&self) -> Result<Vec<DiskInfo>, GathererError> {
        let res: Result<DiskInfoVec, _> = self.call("GetDisksInfo", None);
        res.map(|v| v.into())
    }

    fn get_fans_info(&self) -> Result<Vec<FanInfo>, GathererError> {
        let res: Result<FanInfoVec, _> = self.call("GetFansInfo", None);
        res.map(|v| v.into())
    }

    fn get_gpu_list(&self) -> Result<Vec<Arc<str>>, GathererError> {
        let res: Result<ArcStrVec, _> = self.call("GetGPUList", None);
        res.map(|v| v.into())
    }

    fn get_gpu_static_info(&self) -> Result<Vec<GpuStaticInfo>, GathererError> {
        let res: Result<GpuStaticInfoVec, _> = self.call("GetGPUStaticInfo", None);
        res.map(|v| v.into())
    }

    fn get_gpu_dynamic_info(&self) -> Result<Vec<GpuDynamicInfo>, GathererError> {
        let res: Result<GpuDynamicInfoVec, _> = self.call("GetGPUDynamicInfo", None);
        res.map(|v| v.into())
    }

    fn get_apps(&self) -> Result<HashMap<Arc<str>, App>, GathererError> {
        let res: Result<AppMap, _> = self.call("GetApps", None);
        res.map(|v| v.into())
    }

    fn get_processes(&self) -> Result<HashMap<u32, Process>, GathererError> {
        let res: Result<ProcessMap, _> = self.call("GetProcesses", None);
        res.map(|v| v.into())
    }

    fn get_services(&self) -> Result<HashMap<Arc<str>, Service>, GathererError> {
        let res: Result<ServiceMap, _> = self.call("GetServices", None);
        res.map(|v| v.into())
    }

    fn terminate_process(&self, process_id: u32) -> Result<(), GathererError> {
        self.call("TerminateProcess", Some(&process_id.to_string()))
    }

    fn kill_process(&self, process_id: u32) -> Result<(), GathererError> {
        self.call("KillProcess", Some(&process_id.to_string()))
    }

    fn enable_service(&self, service_name: &str) -> Result<(), GathererError> {
        self.call("EnableService", Some(service_name))
    }

    fn disable_service(&self, service_name: &str) -> Result<(), GathererError> {
        self.call("DisableService", Some(service_name))
    }

    fn start_service(&self, service_name: &str) -> Result<(), GathererError> {
        self.call("StartService", Some(service_name))
    }

    fn stop_service(&self, service_name: &str) -> Result<(), GathererError> {
        self.call("StopService", Some(service_name))
    }

    fn restart_service(&self, service_name: &str) -> Result<(), GathererError> {
        self.call("RestartService", Some(service_name))
    }

    fn get_service_logs(
        &self,
        service_name: &str,
        pid: Option<NonZeroU32>,
    ) -> Result<Arc<str>, GathererError> {
        let arg = format!(
            "{}\x01{}",
            service_name,
            pid.map(|v| v.get()).unwrap_or(0)
        );
        let res: Result<String, _> = self.call("GetServiceLogs", Some(&arg));
        res.map(|v| Arc::<str>::from(v.as_str()))
    }
}
