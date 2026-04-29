#![allow(dead_code)]
use crate::dbus_shim::{Append, Arg, ArgType, IterAppend, Signature};
use std::sync::Arc;
use std::time::Instant;

use crate::platform::cpu_info::*;

use super::{CPU_COUNT, INITIAL_REFRESH_TS, MIN_DELTA_REFRESH};

#[derive(Clone, Debug)]
pub struct MacosCpuStaticInfo {
    name: Arc<str>,
    logical_cpu_count: u32,
    socket_count: Option<u8>,
    base_frequency_khz: Option<u64>,
    virtualization_technology: Option<Arc<str>>,
    is_virtual_machine: Option<bool>,
    l1_combined_cache: Option<u64>,
    l2_cache: Option<u64>,
    l3_cache: Option<u64>,
    l4_cache: Option<u64>,
}

impl Default for MacosCpuStaticInfo {
    fn default() -> Self {
        Self {
            name: Arc::from(""),
            logical_cpu_count: 0,
            socket_count: None,
            base_frequency_khz: None,
            virtualization_technology: None,
            is_virtual_machine: None,
            l1_combined_cache: None,
            l2_cache: None,
            l3_cache: None,
            l4_cache: None,
        }
    }
}

impl CpuStaticInfoExt for MacosCpuStaticInfo {
    fn name(&self) -> &str { self.name.as_ref() }
    fn logical_cpu_count(&self) -> u32 { self.logical_cpu_count }
    fn socket_count(&self) -> Option<u8> { self.socket_count }
    fn base_frequency_khz(&self) -> Option<u64> { self.base_frequency_khz }
    fn virtualization_technology(&self) -> Option<&str> {
        self.virtualization_technology.as_deref()
    }
    fn is_virtual_machine(&self) -> Option<bool> { self.is_virtual_machine }
    fn l1_combined_cache(&self) -> Option<u64> { self.l1_combined_cache }
    fn l2_cache(&self) -> Option<u64> { self.l2_cache }
    fn l3_cache(&self) -> Option<u64> { self.l3_cache }
    fn l4_cache(&self) -> Option<u64> { self.l4_cache }
}

#[derive(Clone, Debug)]
pub struct MacosCpuDynamicInfo {
    overall_utilization_percent: f32,
    overall_kernel_utilization_percent: f32,
    per_logical_cpu_utilization_percent: Vec<f32>,
    per_logical_cpu_kernel_utilization_percent: Vec<f32>,
    current_frequency_mhz: u64,
    temperature: Option<f32>,
    process_count: u64,
    thread_count: u64,
    handle_count: u64,
    uptime_seconds: u64,
}

impl Default for MacosCpuDynamicInfo {
    fn default() -> Self {
        Self {
            overall_utilization_percent: 0.0,
            overall_kernel_utilization_percent: 0.0,
            per_logical_cpu_utilization_percent: vec![],
            per_logical_cpu_kernel_utilization_percent: vec![],
            current_frequency_mhz: 0,
            temperature: None,
            process_count: 0,
            thread_count: 0,
            handle_count: 0,
            uptime_seconds: 0,
        }
    }
}

impl<'a> CpuDynamicInfoExt<'a> for MacosCpuDynamicInfo {
    type Iter = std::slice::Iter<'a, f32>;

    fn overall_utilization_percent(&self) -> f32 { self.overall_utilization_percent }
    fn overall_kernel_utilization_percent(&self) -> f32 { self.overall_kernel_utilization_percent }
    fn per_logical_cpu_utilization_percent(&'a self) -> Self::Iter {
        self.per_logical_cpu_utilization_percent.iter()
    }
    fn per_logical_cpu_kernel_utilization_percent(&'a self) -> Self::Iter {
        self.per_logical_cpu_kernel_utilization_percent.iter()
    }
    fn current_frequency_mhz(&self) -> u64 { self.current_frequency_mhz }
    fn temperature(&self) -> Option<f32> { self.temperature }
    fn process_count(&self) -> u64 { self.process_count }
    fn thread_count(&self) -> u64 { self.thread_count }
    fn handle_count(&self) -> u64 { self.handle_count }
    fn uptime_seconds(&self) -> u64 { self.uptime_seconds }
    fn cpufreq_driver(&self) -> Option<&str> { None }
    fn cpufreq_governor(&self) -> Option<&str> { None }
    fn energy_performance_preference(&self) -> Option<&str> { None }
}

struct CpuTicks {
    user: u64,
    system: u64,
    idle: u64,
    nice: u64,
}

pub struct MacosCpuInfo {
    static_info: MacosCpuStaticInfo,
    dynamic_info: MacosCpuDynamicInfo,
    prev_ticks: Vec<CpuTicks>,
    last_refresh: Instant,
}

impl Default for MacosCpuInfo {
    fn default() -> Self {
        Self {
            static_info: MacosCpuStaticInfo::default(),
            dynamic_info: MacosCpuDynamicInfo::default(),
            prev_ticks: vec![],
            last_refresh: *INITIAL_REFRESH_TS,
        }
    }
}

impl MacosCpuInfo {
    pub fn new() -> Self { Self::default() }
}

impl<'a> CpuInfoExt<'a> for MacosCpuInfo {
    type S = MacosCpuStaticInfo;
    type D = MacosCpuDynamicInfo;
    type P = super::processes::MacosProcesses;

    fn refresh_static_info_cache(&mut self) {
        self.static_info = read_static_info();
    }

    fn refresh_dynamic_info_cache(&mut self, processes: &Self::P) {
        let now = Instant::now();
        if now.duration_since(self.last_refresh) < MIN_DELTA_REFRESH {
            return;
        }
        self.last_refresh = now;
        self.dynamic_info = read_dynamic_info(&mut self.prev_ticks, processes);
    }

    fn static_info(&self) -> &Self::S { &self.static_info }
    fn dynamic_info(&self) -> &Self::D { &self.dynamic_info }
}

fn read_static_info() -> MacosCpuStaticInfo {
    let name = sysctl_string("machdep.cpu.brand_string")
        .unwrap_or_else(|| "Unknown CPU".to_string());

    let logical_count = sysctl_u32("hw.logicalcpu").unwrap_or(1);
    let physical_count = sysctl_u32("hw.physicalcpu").unwrap_or(1);
    let socket_count = (logical_count / physical_count.max(1)).max(1) as u8;

    let base_freq_hz = sysctl_u64("hw.cpufrequency");
    let base_frequency_khz = base_freq_hz.map(|f| f / 1000);

    let l1i = sysctl_u64("hw.l1icachesize").unwrap_or(0);
    let l1d = sysctl_u64("hw.l1dcachesize").unwrap_or(0);
    let l1_combined = if l1i + l1d > 0 { Some(l1i + l1d) } else { None };
    let l2_cache = sysctl_u64("hw.l2cachesize");
    let l3_cache = sysctl_u64("hw.l3cachesize");

    let is_vm = sysctl_u32("kern.hv_vmm_present").map(|v| v != 0);
    let virt_tech = if is_vm == Some(true) {
        Some(Arc::from("Hypervisor"))
    } else {
        None
    };

    MacosCpuStaticInfo {
        name: Arc::from(name.as_str()),
        logical_cpu_count: logical_count,
        socket_count: Some(socket_count),
        base_frequency_khz,
        virtualization_technology: virt_tech,
        is_virtual_machine: is_vm,
        l1_combined_cache: l1_combined,
        l2_cache,
        l3_cache,
        l4_cache: None,
    }
}

fn read_dynamic_info(
    prev_ticks: &mut Vec<CpuTicks>,
    processes: &super::processes::MacosProcesses,
) -> MacosCpuDynamicInfo {
    use crate::platform::ProcessesExt;

    let cpu_count = *CPU_COUNT;

    let new_ticks = read_per_cpu_ticks(cpu_count);

    if prev_ticks.len() != new_ticks.len() {
        *prev_ticks = new_ticks;
        return MacosCpuDynamicInfo {
            per_logical_cpu_utilization_percent: vec![0.0; cpu_count],
            per_logical_cpu_kernel_utilization_percent: vec![0.0; cpu_count],
            ..Default::default()
        };
    }

    let mut per_user = Vec::with_capacity(cpu_count);
    let mut per_kernel = Vec::with_capacity(cpu_count);

    for (prev, curr) in prev_ticks.iter().zip(new_ticks.iter()) {
        let total_delta = (curr.user + curr.system + curr.idle + curr.nice)
            .saturating_sub(prev.user + prev.system + prev.idle + prev.nice)
            as f32;
        let total_delta = total_delta.max(1.0);

        let user_delta = curr.user.saturating_sub(prev.user) as f32;
        let sys_delta = curr.system.saturating_sub(prev.system) as f32;

        per_user.push((user_delta + sys_delta) / total_delta * 100.0);
        per_kernel.push(sys_delta / total_delta * 100.0);
    }

    let overall = per_user.iter().sum::<f32>() / per_user.len().max(1) as f32;
    let overall_kernel = per_kernel.iter().sum::<f32>() / per_kernel.len().max(1) as f32;

    *prev_ticks = new_ticks;

    let freq_mhz = sysctl_u64("hw.cpufrequency")
        .map(|f| f / 1_000_000)
        .unwrap_or_else(|| {
            sysctl_u64("hw.cpufrequency_max")
                .map(|f| f / 1_000_000)
                .unwrap_or(0)
        });

    let uptime = read_uptime_seconds();

    let proc_list = processes.process_list();
    let process_count = proc_list.len() as u64;
    let thread_count = proc_list
        .values()
        .map(|p| {
            use crate::platform::ProcessExt;
            p.task_count() as u64
        })
        .sum();

    MacosCpuDynamicInfo {
        overall_utilization_percent: overall,
        overall_kernel_utilization_percent: overall_kernel,
        per_logical_cpu_utilization_percent: per_user,
        per_logical_cpu_kernel_utilization_percent: per_kernel,
        current_frequency_mhz: freq_mhz,
        temperature: None,
        process_count,
        thread_count,
        handle_count: 0,
        uptime_seconds: uptime,
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
            fn vm_deallocate(
                target_task: u32,
                address: usize,
                size: usize,
            ) -> i32;
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
                .map(|_| CpuTicks { user: 0, system: 0, idle: 0, nice: 0 })
                .collect();
        }

        let slice = std::slice::from_raw_parts(cpu_info_ptr, cpu_info_count as usize);
        let mut ticks = Vec::with_capacity(cpu_count_out as usize);

        for i in 0..cpu_count_out as usize {
            let base = i * CPU_STATE_MAX;
            ticks.push(CpuTicks {
                user: slice[base + CPU_STATE_USER] as u64,
                system: slice[base + CPU_STATE_SYSTEM] as u64,
                idle: slice[base + CPU_STATE_IDLE] as u64,
                nice: slice[base + CPU_STATE_NICE] as u64,
            });
        }

        vm_deallocate(
            mach_task_self_,
            cpu_info_ptr as usize,
            cpu_info_count as usize * std::mem::size_of::<i32>(),
        );

        ticks
    }
}

fn read_uptime_seconds() -> u64 {
    unsafe {
        let mut tv: libc::timeval = std::mem::zeroed();
        let mut size = std::mem::size_of::<libc::timeval>();
        let mib = [libc::CTL_KERN, libc::KERN_BOOTTIME];
        if libc::sysctl(
            mib.as_ptr() as *mut libc::c_int,
            2,
            &mut tv as *mut libc::timeval as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        ) == 0
        {
            let boot = tv.tv_sec as u64;
            let now = libc::time(std::ptr::null_mut()) as u64;
            now.saturating_sub(boot)
        } else {
            0
        }
    }
}

fn sysctl_string(name: &str) -> Option<String> {
    unsafe {
        let cname = std::ffi::CString::new(name).ok()?;
        let mut size: libc::size_t = 0;
        if libc::sysctlbyname(
            cname.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return None;
        }
        let mut buf: Vec<u8> = vec![0u8; size];
        if libc::sysctlbyname(
            cname.as_ptr(),
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        ) != 0
        {
            return None;
        }
        let len = buf.iter().position(|&b| b == 0).unwrap_or(size);
        Some(String::from_utf8_lossy(&buf[..len]).into_owned())
    }
}

fn sysctl_u32(name: &str) -> Option<u32> {
    unsafe {
        let cname = std::ffi::CString::new(name).ok()?;
        let mut val: u32 = 0;
        let mut size = std::mem::size_of::<u32>();
        if libc::sysctlbyname(
            cname.as_ptr(),
            &mut val as *mut u32 as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        ) == 0
        {
            Some(val)
        } else {
            None
        }
    }
}

fn sysctl_u64(name: &str) -> Option<u64> {
    unsafe {
        let cname = std::ffi::CString::new(name).ok()?;
        let mut val: u64 = 0;
        let mut size = std::mem::size_of::<u64>();
        if libc::sysctlbyname(
            cname.as_ptr(),
            &mut val as *mut u64 as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        ) == 0
        {
            Some(val)
        } else {
            None
        }
    }
}

impl Arg for MacosCpuStaticInfo {
    const ARG_TYPE: ArgType = ArgType::Struct;
    fn signature() -> Signature { Signature::from("") }
}
impl Append for MacosCpuStaticInfo {
    fn append_by_ref(&self, _: &mut IterAppend) {}
}

impl Arg for MacosCpuDynamicInfo {
    const ARG_TYPE: ArgType = ArgType::Struct;
    fn signature() -> Signature { Signature::from("") }
}
impl Append for MacosCpuDynamicInfo {
    fn append_by_ref(&self, _: &mut IterAppend) {}
}
