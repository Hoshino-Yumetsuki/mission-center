/* src/gpus/mod.rs
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
use std::ops::DerefMut;
use std::sync::RwLock;
use std::sync::{Arc, OnceLock};

use magpie_platform::gpus::{Gpu, Process, ProcessKind};

use crate::gpu_info_valid;

use nvtop::{
    GpuInfoDynamicInfoValid, GpuInfoProcessInfoValid, GpuInfoStaticInfoValid, GpuProcessType,
};
use run_forked::run_forked;

mod gl_info;
#[allow(unused)]
mod nvtop;
mod run_forked;
mod util;
mod vulkan_info;

pub struct GpuCache {
    gpu_list: Arc<RwLock<nvtop::ListHead>>,
    gpus: HashMap<String, Gpu>,
    proc_refresh_counter: u8,
}

impl magpie_platform::gpus::GpuCache for GpuCache {
    fn new() -> Self
    where
        Self: Sized,
    {
        static GPU_LIST: OnceLock<(usize, Arc<RwLock<nvtop::ListHead>>)> = OnceLock::new();

        let (gpu_count, gpu_list) = GPU_LIST
            .get_or_init(|| {
                unsafe {
                    nvtop::init_extract_gpuinfo_intel();
                    nvtop::init_extract_gpuinfo_amdgpu();
                    nvtop::init_extract_gpuinfo_nvidia();
                    nvtop::init_extract_gpuinfo_v3d();
                    nvtop::init_extract_gpuinfo_msm();
                    nvtop::init_extract_gpuinfo_panfrost();
                    nvtop::init_extract_gpuinfo_panthor();
                }

                let gpu_list = Arc::new(RwLock::new(nvtop::ListHead {
                    next: std::ptr::null_mut(),
                    prev: std::ptr::null_mut(),
                }));
                {
                    let mut gl = gpu_list.write().unwrap();
                    gl.next = gl.deref_mut();
                    gl.prev = gl.deref_mut();
                }

                let gpu_count = {
                    let mut gpu_list = gpu_list.write().unwrap();
                    let gpu_list = gpu_list.deref_mut();

                    let mut gpu_count: u32 = 0;
                    let nvt_result =
                        unsafe { nvtop::gpuinfo_init_info_extraction(&mut gpu_count, gpu_list) };
                    if nvt_result == 0 {
                        log::error!("Unable to initialize GPU info extraction");
                        gpu_count = 0;
                    }

                    let nvt_result = unsafe { nvtop::gpuinfo_populate_static_infos(gpu_list) };
                    if nvt_result == 0 {
                        log::error!("Unable to populate static GPU info");
                    }

                    gpu_count
                };

                (gpu_count as usize, gpu_list)
            })
            .clone();

        let mut gpus = HashMap::with_capacity(gpu_count);

        {
            let mut gpu_list = gpu_list.write().unwrap();
            let gpu_list = gpu_list.deref_mut();

            let mut device = gpu_list.next;
            while device != gpu_list {
                let dev = unsafe { &*(device as *const nvtop::GpuInfo) };
                device = unsafe { (*device).next };

                let Some(pci_id) = util::pci_id(dev) else {
                    continue;
                };

                let Some((vendor_id, device_id)) =
                    util::ven_dev_ids(&pci_id, &mut String::with_capacity(1024))
                else {
                    continue;
                };

                let device_name =
                    if gpu_info_valid!(dev.static_info, GpuInfoStaticInfoValid::DeviceNameValid) {
                        Some(
                            unsafe {
                                std::ffi::CStr::from_ptr(dev.static_info.device_name.as_ptr())
                            }
                            .to_string_lossy()
                            .to_string(),
                        )
                    } else {
                        None
                    };

                let gpu = Gpu {
                    id: pci_id.to_string(),
                    vendor_id,
                    device_id,
                    device_name,
                    encode_decode_shared: dev.static_info.encode_decode_shared,
                    ..Default::default()
                };
                gpus.insert(pci_id.to_string(), gpu);
            }
        }

        let vulkan_versions = unsafe {
            run_forked(|| {
                if let Some(vulkan_info) = vulkan_info::VulkanInfo::new() {
                    Ok(vulkan_info
                        .supported_vulkan_versions()
                        .unwrap_or(HashMap::new()))
                } else {
                    Ok(HashMap::new())
                }
            })
        };
        let vulkan_versions = vulkan_versions.unwrap_or_else(|e| {
            log::warn!("Failed to get Vulkan information: {}", e);
            HashMap::new()
        });

        for (pci_id, gpu) in &mut gpus {
            if let Some((variant, version)) = unsafe {
                run_forked(|| Ok(gl_info::supported_opengl_version(pci_id))).unwrap_or_else(|e| {
                    log::warn!("Failed to get OpenGL information: {}", e);
                    None
                })
            } {
                gpu.opengl_variant = Some(variant as i32);
                gpu.opengl_version = Some(version);
            }

            let device_id = ((gpu.vendor_id) << 16) | gpu.device_id & 0x0000FFFF;
            if let Some(vulkan_version) = vulkan_versions.get(&device_id) {
                gpu.vulkan_version = Some(*vulkan_version);
            }
        }

        Self {
            gpu_list,
            gpus,
            proc_refresh_counter: 0,
        }
    }

    fn refresh(&mut self) {
        let mut gpu_list = self.gpu_list.write().unwrap();
        let gpu_list = gpu_list.deref_mut();

        let result = unsafe { nvtop::gpuinfo_refresh_dynamic_info(gpu_list) };
        if result == 0 {
            log::error!("Unable to refresh dynamic GPU info");
            return;
        }

        // Refreshing processes is very expensive, so refresh them only every 3rd time
        if self.proc_refresh_counter == 0 {
            let result = unsafe { nvtop::gpuinfo_refresh_processes(gpu_list) };
            if result == 0 {
                log::warn!("Unable to refresh GPU processes");
            }
        } else {
            self.proc_refresh_counter += 1;
            self.proc_refresh_counter %= 3;
        }

        let result = unsafe { nvtop::gpuinfo_utilisation_rate(gpu_list) };
        if result == 0 {
            log::warn!("Unable to refresh utilization rate");
        }

        let result = unsafe { nvtop::gpuinfo_fix_dynamic_info_from_process_info(gpu_list) };
        if result == 0 {
            log::warn!("Unable to fix dynamic GPU info from process info");
        }

        let mut device: *mut nvtop::ListHead = gpu_list.next;
        while device != gpu_list {
            let dev = unsafe { &*(device as *const nvtop::GpuInfo) };
            device = unsafe { (*device).next };

            let Some(pci_id) = util::pci_id(dev) else {
                continue;
            };

            let gpu = match self.gpus.get_mut(pci_id.as_str()) {
                Some(gpu) => gpu,
                None => {
                    log::warn!("Unable to find gpu `{}` in cache", pci_id);
                    continue;
                }
            };

            gpu.temperature_c =
                if gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::GpuTempValid) {
                    Some(dev.dynamic_info.gpu_temp as f32)
                } else {
                    None
                };
            gpu.fan_speed_percent =
                if gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::FanSpeedValid) {
                    Some(dev.dynamic_info.fan_speed as f32)
                } else {
                    None
                };
            gpu.utilization_percent =
                if gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::GpuUtilRateValid) {
                    Some(dev.dynamic_info.gpu_util_rate as f32)
                } else {
                    None
                };
            gpu.power_draw_watts =
                if gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::PowerDrawValid) {
                    Some(dev.dynamic_info.power_draw as f32 / 1000.)
                } else {
                    None
                };
            gpu.max_power_draw_watts =
                if gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::PowerDrawMaxValid) {
                    Some(dev.dynamic_info.power_draw_max as f32 / 1000.)
                } else {
                    None
                };
            gpu.clock_speed_mhz = if gpu_info_valid!(
                dev.dynamic_info,
                GpuInfoDynamicInfoValid::GpuClockSpeedValid
            ) {
                Some(dev.dynamic_info.gpu_clock_speed)
            } else {
                None
            };
            gpu.max_clock_speed_mhz = if gpu_info_valid!(
                dev.dynamic_info,
                GpuInfoDynamicInfoValid::GpuClockSpeedMaxValid
            ) {
                Some(dev.dynamic_info.gpu_clock_speed_max)
            } else {
                None
            };
            gpu.memory_speed_mhz = if gpu_info_valid!(
                dev.dynamic_info,
                GpuInfoDynamicInfoValid::MemClockSpeedValid
            ) {
                Some(dev.dynamic_info.mem_clock_speed)
            } else {
                None
            };
            gpu.max_memory_speed_mhz = if gpu_info_valid!(
                dev.dynamic_info,
                GpuInfoDynamicInfoValid::MemClockSpeedMaxValid
            ) {
                Some(dev.dynamic_info.mem_clock_speed_max)
            } else {
                None
            };
            gpu.total_memory =
                if gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::FreeMemoryValid) {
                    Some(dev.dynamic_info.total_memory)
                } else {
                    None
                };
            gpu.used_memory =
                if gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::UsedMemoryValid) {
                    Some(dev.dynamic_info.used_memory)
                } else {
                    None
                };
            gpu.total_shared_memory = util::shared_mem_total(&pci_id, &mut String::new());
            gpu.used_shared_memory = util::shared_mem_used(&pci_id, &mut String::new());
            gpu.encoder_percent = {
                // FIXME: Concession, if the value is 0, we assume it might be valid even if the
                //        validity check fails.
                //        This is needed because these values are not set to valid until the first
                //        time the encoding function of the GPU is used

                let valid =
                    gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::EncoderRateValid);

                if valid || dev.dynamic_info.encoder_rate == 0 {
                    Some(dev.dynamic_info.encoder_rate as f32)
                } else {
                    None
                }
            };
            gpu.decoder_percent = {
                // FIXME: Concession, if the value is 0, we assume it might be valid even if the
                //        validity check fails.
                //        This is needed because these values are not set to valid until the first
                //        time the decoding function of the GPU is used

                let valid =
                    gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::DecoderRateValid);

                if valid || dev.dynamic_info.decoder_rate == 0 {
                    Some(dev.dynamic_info.decoder_rate as f32)
                } else {
                    None
                }
            };
            gpu.pcie_gen =
                if gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::PcieLinkGenValid) {
                    Some(dev.dynamic_info.pcie_link_gen)
                } else {
                    util::pcie_speed(&pci_id, &mut String::new())
                };

            gpu.pcie_lanes = if gpu_info_valid!(
                dev.dynamic_info,
                GpuInfoDynamicInfoValid::PcieLinkWidthValid
            ) {
                Some(dev.dynamic_info.pcie_link_width)
            } else {
                util::pcie_width(&pci_id, &mut String::new())
            };

            gpu.max_pcie_gen = util::max_pcie_speed(&pci_id, &mut String::new());
            gpu.max_pcie_lanes = util::max_pcie_width(&pci_id, &mut String::new());

            if let Some(max_pcie_gen) = gpu.max_pcie_gen {
                if let Some(pcie_gen) = gpu.pcie_gen {
                    if let Some(max_pcie_lanes) = gpu.max_pcie_lanes {
                        if let Some(pcie_lanes) = gpu.pcie_lanes {
                            if max_pcie_gen < pcie_gen || max_pcie_lanes < pcie_lanes {
                                gpu.max_pcie_gen = None;
                                gpu.max_pcie_lanes = None;
                            }
                        }
                    }
                }
            }

            gpu.processes.clear();
            for i in 0..dev.processes_count as usize {
                let nvtop_proc = unsafe { &*dev.processes.add(i) };

                let process = Process {
                    kind: match nvtop_proc.r#type {
                        GpuProcessType::Graphical => Some(ProcessKind::Graphical as i32),
                        GpuProcessType::Compute => Some(ProcessKind::Compute as i32),
                        GpuProcessType::GraphicalCompute => {
                            Some(ProcessKind::GraphicalCompute as i32)
                        }
                        _ => None,
                    },
                    memory_usage_bytes: if gpu_info_valid!(
                        nvtop_proc,
                        GpuInfoProcessInfoValid::GpuMemoryUsageValid
                    ) {
                        Some(nvtop_proc.gpu_memory_usage)
                    } else {
                        None
                    },
                    gpu_usage_percent: if gpu_info_valid!(
                        nvtop_proc,
                        GpuInfoProcessInfoValid::GpuUsageValid
                    ) {
                        Some(nvtop_proc.gpu_usage as f32)
                    } else {
                        None
                    },
                    encode_usage_percent: if gpu_info_valid!(
                        nvtop_proc,
                        GpuInfoProcessInfoValid::EncodeUsageValid
                    ) {
                        Some(nvtop_proc.encode_usage as f32)
                    } else {
                        None
                    },
                    decode_usage_percent: if gpu_info_valid!(
                        nvtop_proc,
                        GpuInfoProcessInfoValid::DecodeUsageValid
                    ) {
                        Some(nvtop_proc.decode_usage as f32)
                    } else {
                        None
                    },
                };
                gpu.processes.insert(nvtop_proc.pid as u32, process);
            }
        }
    }

    fn cached_entries(&self) -> &HashMap<String, Gpu> {
        &self.gpus
    }
}
