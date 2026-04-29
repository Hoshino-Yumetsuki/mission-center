#![allow(dead_code)]
use crate::dbus_shim::{Append, Arg, ArgType, IterAppend, Signature};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crate::platform::{
    ApiVersion, GpuDynamicInfoExt, GpuInfoExt, GpuStaticInfoExt, OpenGLApiVersion,
};

use super::{INITIAL_REFRESH_TS, MIN_DELTA_REFRESH};

#[derive(Debug, Clone)]
pub struct MacosGpuStaticInfo {
    id: Arc<str>,
    device_name: Arc<str>,
    vendor_id: u16,
    device_id: u16,
    total_memory: u64,
    total_gtt: u64,
    opengl_version: Option<OpenGLApiVersion>,
    vulkan_version: Option<ApiVersion>,
    metal_version: Option<ApiVersion>,
    pcie_gen: u8,
    pcie_lanes: u8,
}

impl Default for MacosGpuStaticInfo {
    fn default() -> Self {
        Self {
            id: Arc::from(""),
            device_name: Arc::from(""),
            vendor_id: 0,
            device_id: 0,
            total_memory: 0,
            total_gtt: 0,
            opengl_version: None,
            vulkan_version: None,
            metal_version: None,
            pcie_gen: 0,
            pcie_lanes: 0,
        }
    }
}

impl GpuStaticInfoExt for MacosGpuStaticInfo {
    fn id(&self) -> &str { self.id.as_ref() }
    fn device_name(&self) -> &str { self.device_name.as_ref() }
    fn vendor_id(&self) -> u16 { self.vendor_id }
    fn device_id(&self) -> u16 { self.device_id }
    fn total_memory(&self) -> u64 { self.total_memory }
    fn total_gtt(&self) -> u64 { self.total_gtt }
    fn opengl_version(&self) -> Option<&OpenGLApiVersion> { self.opengl_version.as_ref() }
    fn vulkan_version(&self) -> Option<&ApiVersion> { self.vulkan_version.as_ref() }
    fn metal_version(&self) -> Option<&ApiVersion> { self.metal_version.as_ref() }
    fn direct3d_version(&self) -> Option<&ApiVersion> { None }
    fn pcie_gen(&self) -> u8 { self.pcie_gen }
    fn pcie_lanes(&self) -> u8 { self.pcie_lanes }
}

#[derive(Debug, Clone)]
pub struct MacosGpuDynamicInfo {
    id: Arc<str>,
    temp_celsius: u32,
    fan_speed_percent: u32,
    util_percent: u32,
    power_draw_watts: f32,
    power_draw_max_watts: f32,
    clock_speed_mhz: u32,
    clock_speed_max_mhz: u32,
    mem_speed_mhz: u32,
    mem_speed_max_mhz: u32,
    free_memory: u64,
    used_memory: u64,
    used_gtt: u64,
    encoder_percent: u32,
    decoder_percent: u32,
}

impl Default for MacosGpuDynamicInfo {
    fn default() -> Self {
        Self {
            id: Arc::from(""),
            temp_celsius: 0,
            fan_speed_percent: 0,
            util_percent: 0,
            power_draw_watts: 0.0,
            power_draw_max_watts: 0.0,
            clock_speed_mhz: 0,
            clock_speed_max_mhz: 0,
            mem_speed_mhz: 0,
            mem_speed_max_mhz: 0,
            free_memory: 0,
            used_memory: 0,
            used_gtt: 0,
            encoder_percent: 0,
            decoder_percent: 0,
        }
    }
}

impl GpuDynamicInfoExt for MacosGpuDynamicInfo {
    fn id(&self) -> &str { self.id.as_ref() }
    fn temp_celsius(&self) -> u32 { self.temp_celsius }
    fn fan_speed_percent(&self) -> u32 { self.fan_speed_percent }
    fn util_percent(&self) -> u32 { self.util_percent }
    fn power_draw_watts(&self) -> f32 { self.power_draw_watts }
    fn power_draw_max_watts(&self) -> f32 { self.power_draw_max_watts }
    fn clock_speed_mhz(&self) -> u32 { self.clock_speed_mhz }
    fn clock_speed_max_mhz(&self) -> u32 { self.clock_speed_max_mhz }
    fn mem_speed_mhz(&self) -> u32 { self.mem_speed_mhz }
    fn mem_speed_max_mhz(&self) -> u32 { self.mem_speed_max_mhz }
    fn free_memory(&self) -> u64 { self.free_memory }
    fn used_memory(&self) -> u64 { self.used_memory }
    fn used_gtt(&self) -> u64 { self.used_gtt }
    fn encoder_percent(&self) -> u32 { self.encoder_percent }
    fn decoder_percent(&self) -> u32 { self.decoder_percent }
}

pub struct MacosGpuInfo {
    gpu_ids: Vec<Arc<str>>,
    static_info: HashMap<Arc<str>, MacosGpuStaticInfo>,
    dynamic_info: HashMap<Arc<str>, MacosGpuDynamicInfo>,
    last_static_refresh: Instant,
    last_dynamic_refresh: Instant,
}

impl Default for MacosGpuInfo {
    fn default() -> Self {
        Self {
            gpu_ids: vec![],
            static_info: HashMap::new(),
            dynamic_info: HashMap::new(),
            last_static_refresh: *INITIAL_REFRESH_TS,
            last_dynamic_refresh: *INITIAL_REFRESH_TS,
        }
    }
}

impl MacosGpuInfo {
    pub fn new() -> Self { Self::default() }
}

impl<'a> GpuInfoExt<'a> for MacosGpuInfo {
    type S = MacosGpuStaticInfo;
    type D = MacosGpuDynamicInfo;
    type P = crate::platform::platform_impl::processes::MacosProcesses;
    type Iter = std::iter::Map<std::slice::Iter<'a, Arc<str>>, fn(&'a Arc<str>) -> &'a str>;

    fn refresh_gpu_list(&mut self) {
        let gpus = enumerate_gpus();
        self.gpu_ids = gpus.iter().map(|g| g.id.clone()).collect();
        for gpu in gpus {
            let id = gpu.id.clone();
            self.static_info.insert(id.clone(), gpu);
            self.dynamic_info
                .entry(id.clone())
                .or_insert_with(|| MacosGpuDynamicInfo {
                    id: id.clone(),
                    ..Default::default()
                });
        }
    }

    fn refresh_static_info_cache(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_static_refresh) < MIN_DELTA_REFRESH {
            return;
        }
        self.last_static_refresh = now;
        self.refresh_gpu_list();
    }

    fn refresh_dynamic_info_cache(&mut self, _processes: &mut Self::P) {
        let now = Instant::now();
        if now.duration_since(self.last_dynamic_refresh) < MIN_DELTA_REFRESH {
            return;
        }
        self.last_dynamic_refresh = now;

        for id in &self.gpu_ids {
            let stats = read_ioaccelerator_stats(id.as_ref());
            if let Some(entry) = self.dynamic_info.get_mut(id) {
                entry.util_percent = stats.util_percent;
                entry.clock_speed_mhz = stats.clock_mhz;
                entry.mem_speed_mhz = stats.mem_clock_mhz;
                entry.used_memory = stats.used_vram;
                entry.free_memory = stats.free_vram;
                entry.encoder_percent = stats.encoder_percent;
                entry.decoder_percent = stats.decoder_percent;
            }
        }
    }

    fn enumerate(&'a self) -> Self::Iter {
        self.gpu_ids.iter().map(|s| s.as_ref())
    }

    fn static_info(&self, id: &str) -> Option<&Self::S> {
        self.static_info.get(id)
    }

    fn dynamic_info(&self, id: &str) -> Option<&Self::D> {
        self.dynamic_info.get(id)
    }
}

fn enumerate_gpus() -> Vec<MacosGpuStaticInfo> {
    let output = std::process::Command::new("/usr/sbin/system_profiler")
        .args(["SPDisplaysDataType", "-json"])
        .output();

    let total_mem = read_total_memory();

    match output {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            parse_system_profiler_gpus(&text, total_mem)
        }
        _ => vec![],
    }
}

fn read_total_memory() -> u64 {
    unsafe {
        let mut mem_size: u64 = 0;
        let mut size = std::mem::size_of::<u64>();
        libc::sysctlbyname(
            b"hw.memsize\0".as_ptr() as *const libc::c_char,
            &mut mem_size as *mut u64 as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        );
        mem_size
    }
}

fn parse_system_profiler_gpus(json: &str, total_mem: u64) -> Vec<MacosGpuStaticInfo> {
    let mut gpus = vec![];

    let parsed: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return gpus,
    };

    let displays = match parsed.get("SPDisplaysDataType").and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return gpus,
    };

    for (idx, display) in displays.iter().enumerate() {
        let name = display
            .get("sppci_model")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown GPU")
            .to_string();

        let vram_bytes = display
            .get("spdisplays_vram")
            .and_then(|v| v.as_str())
            .map(parse_vram_string)
            .filter(|&v| v > 0)
            .unwrap_or(total_mem);

        let vendor_id = display
            .get("spdisplays_vendor")
            .or_else(|| display.get("sppci_vendor"))
            .and_then(|v| v.as_str())
            .map(parse_vendor_id)
            .unwrap_or(0);

        let device_id = display
            .get("sppci_device_id")
            .and_then(|v| v.as_str())
            .map(|s| u16::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or(0))
            .unwrap_or(0);

        let metal_version = detect_metal_version(display);

        let id = format!("gpu-{}", idx);

        gpus.push(MacosGpuStaticInfo {
            id: Arc::from(id.as_str()),
            device_name: Arc::from(name.as_str()),
            vendor_id,
            device_id,
            total_memory: vram_bytes,
            total_gtt: 0,
            opengl_version: None,
            vulkan_version: None,
            metal_version,
            pcie_gen: 0,
            pcie_lanes: 0,
        });
    }

    gpus
}

fn parse_vram_string(s: &str) -> u64 {
    let s = s.to_lowercase();
    let num: f64 = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect::<String>()
        .parse()
        .unwrap_or(0.0);
    if s.contains("gb") {
        (num * 1024.0 * 1024.0 * 1024.0) as u64
    } else if s.contains("mb") {
        (num * 1024.0 * 1024.0) as u64
    } else {
        num as u64
    }
}

fn parse_vendor_id(s: &str) -> u16 {
    let lower = s.to_lowercase();
    if lower.contains("apple") {
        0x106b
    } else if lower.contains("amd") || lower.contains("ati") {
        0x1002
    } else if lower.contains("nvidia") {
        0x10de
    } else if lower.contains("intel") {
        0x8086
    } else {
        0
    }
}

fn detect_metal_version(display: &serde_json::Value) -> Option<ApiVersion> {
    let metal_family = display
        .get("spdisplays_metal")
        .and_then(|v| v.as_str())?;

    if metal_family.contains("Metal 3") || metal_family.contains("GPUFamily Apple 9") {
        Some(ApiVersion { major: 3, minor: 0, patch: 0 })
    } else if metal_family.contains("Metal 2") || metal_family.contains("GPUFamily Apple") {
        Some(ApiVersion { major: 2, minor: 0, patch: 0 })
    } else if metal_family.contains("Metal") {
        Some(ApiVersion { major: 1, minor: 0, patch: 0 })
    } else {
        None
    }
}

struct AcceleratorStats {
    util_percent: u32,
    clock_mhz: u32,
    mem_clock_mhz: u32,
    used_vram: u64,
    free_vram: u64,
    encoder_percent: u32,
    decoder_percent: u32,
}

fn read_ioaccelerator_stats(_gpu_id: &str) -> AcceleratorStats {
    let output = std::process::Command::new("/usr/sbin/ioreg")
        .args(["-r", "-d", "1", "-c", "IOAccelerator"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            parse_ioreg_gpu(&text)
        }
        _ => AcceleratorStats {
            util_percent: 0,
            clock_mhz: 0,
            mem_clock_mhz: 0,
            used_vram: 0,
            free_vram: 0,
            encoder_percent: 0,
            decoder_percent: 0,
        },
    }
}

fn parse_ioreg_gpu(text: &str) -> AcceleratorStats {
    let mut util_percent = 0u32;
    let mut used_vram: u64 = 0;

    for line in text.lines() {
        let line = line.trim();
        if line.contains("PerformanceStatistics") {
            if let Some(val) = extract_ioreg_u64(line, "Device Utilization %") {
                util_percent = val as u32;
            }
            if let Some(val) = extract_ioreg_u64(line, "In use system memory") {
                used_vram = val;
            }
        }
    }

    AcceleratorStats {
        util_percent,
        clock_mhz: 0,
        mem_clock_mhz: 0,
        used_vram,
        free_vram: 0,
        encoder_percent: 0,
        decoder_percent: 0,
    }
}

fn extract_ioreg_u64(line: &str, key: &str) -> Option<u64> {
    let search = format!("\"{}\"=", key);
    let pos = line.find(&search)?;
    let rest = &line[pos + search.len()..];
    let num_str: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    num_str.parse().ok()
}

impl Arg for MacosGpuStaticInfo {
    const ARG_TYPE: ArgType = ArgType::Struct;
    fn signature() -> Signature { Signature::from("") }
}
impl Append for MacosGpuStaticInfo {
    fn append_by_ref(&self, _: &mut IterAppend) {}
}

impl Arg for MacosGpuDynamicInfo {
    const ARG_TYPE: ArgType = ArgType::Struct;
    fn signature() -> Signature { Signature::from("") }
}
impl Append for MacosGpuDynamicInfo {
    fn append_by_ref(&self, _: &mut IterAppend) {}
}
