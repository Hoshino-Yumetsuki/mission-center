/* src/gpus/mod.rs
 *
 * Copyright 2026 Mission Center Developers
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
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::sync::{Arc, OnceLock};

use magpie_platform::gpus::{Gpu, Process, ProcessKind};

use crate::gpu_info_valid;

use drm::DrmDevice;
use nvtop::{
    GpuInfoDynamicInfoValid, GpuInfoProcessInfoValid, GpuInfoStaticInfoValid, GpuProcessType,
};
use run_forked::run_forked;
use vulkan_info::VulkanDevice;

mod drm;
mod gl_info;
#[allow(unused)]
mod nvtop;
mod run_forked;
mod util;
mod vulkan_info;

struct GpuDevice {
    id: String,
    sysfs_device: Option<PathBuf>,
    is_pci: bool,
    drm: Option<DrmDevice>,
}

impl GpuDevice {
    /// Nodes to try when probing OpenGL support, least privileged first.
    fn render_nodes(&self) -> Vec<PathBuf> {
        match &self.drm {
            // Driver ioctls on the card node require DRM authentication, the render node does not
            Some(drm) => [drm.render_node.clone(), Some(drm.card_node.clone())]
                .into_iter()
                .flatten()
                .collect(),
            // No DRM device was resolved, so fall back to the udev by-path symlink
            None if self.is_pci => vec![PathBuf::from(format!(
                "/dev/dri/by-path/pci-{}-card",
                self.id
            ))],
            None => vec![],
        }
    }

    fn is_vulkan_device(&self, vulkan: &VulkanDevice) -> bool {
        let Some(drm) = self.drm.as_ref() else {
            return false;
        };

        vulkan.primary_node.is_some_and(|node| node == drm.card_dev)
            || (vulkan.render_node.is_some() && vulkan.render_node == drm.render_dev)
    }
}

fn match_device(drm_devices: &mut [DrmDevice], pdev: &str, backend: &str) -> Option<usize> {
    if !pdev.is_empty() {
        return drm_devices
            .iter()
            .position(|d| d.sysname.eq_ignore_ascii_case(pdev));
    }

    // nvtop identifies SoC GPUs by the bus driver name (v3d) or by the DRM driver name (msm,
    // panfrost, panthor), so try the free comparison before opening any device node
    if let Some(index) = drm_devices.iter().position(|d| d.driver == backend) {
        return Some(index);
    }

    (0..drm_devices.len()).find(|&i| drm_devices[i].drm_driver() == Some(backend))
}

fn is_pci_address(address: &str) -> bool {
    fn hex_digits(part: &str, lengths: std::ops::RangeInclusive<usize>) -> bool {
        lengths.contains(&part.len()) && part.bytes().all(|b| b.is_ascii_hexdigit())
    }

    let Some((domain, rest)) = address.split_once(':') else {
        return false;
    };
    let Some((bus, rest)) = rest.split_once(':') else {
        return false;
    };
    let Some((device, function)) = rest.split_once('.') else {
        return false;
    };

    // `dddd:bb:dd.f`
    hex_digits(domain, 4..=8)
        && hex_digits(bus, 2..=2)
        && hex_digits(device, 2..=2)
        && hex_digits(function, 1..=1)
}

fn resolve_devices(gpu_list: &mut nvtop::ListHead) -> HashMap<usize, GpuDevice> {
    let mut drm_devices = drm::enumerate();
    let mut resolved = HashMap::new();
    let mut unidentified = 0;

    let mut device = gpu_list.next;
    while device != gpu_list {
        let dev = unsafe { &*(device as *const nvtop::GpuInfo) };
        let entry = device as usize;
        device = unsafe { (*device).next };

        let pdev = c_str(&dev.pdev);
        let backend = vendor_name(dev);

        let gpu_device = match match_device(&mut drm_devices, pdev.as_ref(), backend)
            .map(|i| drm_devices.remove(i))
        {
            Some(drm) => GpuDevice {
                id: drm.sysname.clone(),
                sysfs_device: Some(drm.sysfs_device.clone()),
                is_pci: drm.is_pci(),
                drm: Some(drm),
            },
            // A PCI GPU with no DRM card, an NVIDIA card without `nvidia-drm` loaded
            // for instance
            None if is_pci_address(&pdev) => {
                let pdev = pdev.to_ascii_lowercase();
                GpuDevice {
                    sysfs_device: Some(PathBuf::from(format!("/sys/bus/pci/devices/{pdev}"))),
                    id: pdev,
                    is_pci: true,
                    drm: None,
                }
            }
            None => {
                unidentified += 1;
                log::warn!(
                    "No DRM device matches the `{backend}` GPU reported by nvtop, pdev `{pdev}`"
                );
                GpuDevice {
                    id: format!("{backend}-{unidentified}"),
                    sysfs_device: None,
                    is_pci: false,
                    drm: None,
                }
            }
        };

        log::debug!(
            "GPU `{}`: nvtop backend `{}`, driver `{}`, bus `{}`, node `{}`",
            gpu_device.id,
            backend,
            gpu_device.drm.as_ref().map_or("", |d| d.driver.as_str()),
            gpu_device
                .drm
                .as_ref()
                .map_or("none".into(), |d| d.bus.to_string()),
            gpu_device
                .drm
                .as_ref()
                .map_or_else(|| "none".into(), |d| d.card_node.display().to_string()),
        );

        resolved.insert(entry, gpu_device);
    }

    resolved
}

fn c_str(field: &[std::ffi::c_char]) -> std::borrow::Cow<'_, str> {
    let bytes = unsafe { &*(field as *const [std::ffi::c_char] as *const [u8]) };
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());

    String::from_utf8_lossy(&bytes[..end])
}

fn vendor_name(dev: &nvtop::GpuInfo) -> &str {
    if dev.vendor.is_null() || unsafe { (*dev.vendor).name.is_null() } {
        return "";
    }

    unsafe { std::ffi::CStr::from_ptr((*dev.vendor).name) }
        .to_str()
        .unwrap_or_default()
}

pub struct GpuCache {
    gpu_list: Arc<RwLock<nvtop::ListHead>>,
    devices: HashMap<usize, GpuDevice>,
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
        let mut buffer = String::with_capacity(1024);

        let devices = {
            let mut gpu_list = gpu_list.write().unwrap();
            let gpu_list = gpu_list.deref_mut();

            let devices = resolve_devices(gpu_list);

            let mut device = gpu_list.next;
            while device != gpu_list {
                let dev = unsafe { &*(device as *const nvtop::GpuInfo) };
                let entry = device as usize;
                device = unsafe { (*device).next };

                let Some(gpu_device) = devices.get(&entry) else {
                    continue;
                };

                let (vendor_id, device_id) = gpu_device
                    .sysfs_device
                    .as_deref()
                    .and_then(|device| util::ven_dev_ids(device, &mut buffer))
                    .unwrap_or_else(|| {
                        if gpu_device.is_pci {
                            log::warn!(
                                "Unable to read the vendor and device IDs of GPU `{}`",
                                gpu_device.id
                            );
                        }
                        (0, 0)
                    });

                let device_name =
                    if gpu_info_valid!(dev.static_info, GpuInfoStaticInfoValid::DeviceNameValid) {
                        Some(c_str(&dev.static_info.device_name).into_owned())
                    } else {
                        None
                    };

                let gpu = Gpu {
                    id: gpu_device.id.clone(),
                    vendor_id,
                    device_id,
                    device_name,
                    encode_decode_shared: dev.static_info.encode_decode_shared,
                    ..Default::default()
                };
                gpus.insert(gpu_device.id.clone(), gpu);
            }

            devices
        };

        let vulkan_devices = unsafe {
            run_forked(|| {
                Ok(vulkan_info::VulkanInfo::new()
                    .map(|vulkan_info| vulkan_info.devices())
                    .unwrap_or_default())
            })
        };
        let vulkan_devices = vulkan_devices.unwrap_or_else(|e| {
            log::warn!("Failed to get Vulkan information: {}", e);
            vec![]
        });

        for gpu_device in devices.values() {
            let Some(gpu) = gpus.get_mut(&gpu_device.id) else {
                continue;
            };

            let nodes = gpu_device.render_nodes();
            let nodes = nodes.iter().map(PathBuf::as_path).collect::<Vec<_>>();
            if let Some((variant, version)) =
                unsafe { run_forked(|| Ok(gl_info::supported_opengl_version(&nodes))) }
                    .unwrap_or_else(|e| {
                        log::warn!("Failed to get OpenGL information: {}", e);
                        None
                    })
            {
                gpu.opengl_variant = Some(variant as i32);
                gpu.opengl_version = Some(version);
            }

            let ids_key = (gpu.vendor_id << 16) | (gpu.device_id & 0x0000FFFF);
            let vulkan = vulkan_devices
                .iter()
                .find(|vulkan| gpu_device.is_vulkan_device(vulkan))
                .or_else(|| {
                    vulkan_devices
                        .iter()
                        .find(|vulkan| ids_key != 0 && vulkan.ids_key == ids_key)
                });
            if let Some(vulkan) = vulkan {
                gpu.vulkan_version = Some(vulkan.version);
            }
        }

        Self {
            gpu_list,
            devices,
            gpus,
            proc_refresh_counter: 2, // Start at 2 so that the first increment will refresh processes
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
        self.proc_refresh_counter = (self.proc_refresh_counter + 1) % 3;
        if self.proc_refresh_counter == 0 {
            let result = unsafe { nvtop::gpuinfo_refresh_processes(gpu_list) };
            if result == 0 {
                log::warn!("Unable to refresh GPU processes");
            }
        }

        let result = unsafe { nvtop::gpuinfo_utilisation_rate(gpu_list) };
        if result == 0 {
            log::warn!("Unable to refresh utilization rate");
        }

        let result = unsafe { nvtop::gpuinfo_fix_dynamic_info_from_process_info(gpu_list) };
        if result == 0 {
            log::warn!("Unable to fix dynamic GPU info from process info");
        }

        let mut buffer = String::with_capacity(1024);

        let mut device: *mut nvtop::ListHead = gpu_list.next;
        while device != gpu_list {
            let dev = unsafe { &*(device as *const nvtop::GpuInfo) };
            let entry = device as usize;
            device = unsafe { (*device).next };

            let Some(gpu_device) = self.devices.get(&entry) else {
                continue;
            };
            let sysfs_device: Option<&Path> = gpu_device.sysfs_device.as_deref();

            let gpu = match self.gpus.get_mut(gpu_device.id.as_str()) {
                Some(gpu) => gpu,
                None => {
                    log::warn!("Unable to find gpu `{}` in cache", gpu_device.id);
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
                if gpu_info_valid!(dev.dynamic_info, GpuInfoDynamicInfoValid::TotalMemoryValid) {
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
            gpu.total_shared_memory =
                sysfs_device.and_then(|device| util::shared_mem_total(device, &mut buffer));
            gpu.used_shared_memory =
                sysfs_device.and_then(|device| util::shared_mem_used(device, &mut buffer));
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
                    sysfs_device.and_then(|device| util::pcie_speed(device, &mut buffer))
                };

            gpu.pcie_lanes = if gpu_info_valid!(
                dev.dynamic_info,
                GpuInfoDynamicInfoValid::PcieLinkWidthValid
            ) {
                Some(dev.dynamic_info.pcie_link_width)
            } else {
                sysfs_device.and_then(|device| util::pcie_width(device, &mut buffer))
            };

            gpu.max_pcie_gen =
                sysfs_device.and_then(|device| util::max_pcie_speed(device, &mut buffer));
            gpu.max_pcie_lanes =
                sysfs_device.and_then(|device| util::max_pcie_width(device, &mut buffer));

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
                        GpuProcessType::GRAPHICAL => Some(ProcessKind::Graphical as i32),
                        GpuProcessType::COMPUTE => Some(ProcessKind::Compute as i32),
                        GpuProcessType::GRAPHICAL_COMPUTE => {
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::drm::{Bus, DrmDevice};
    use super::*;

    fn drm_device(sysname: &str, bus: Bus, driver: &str, card_index: u32) -> DrmDevice {
        DrmDevice {
            card_index,
            sysname: sysname.to_owned(),
            bus,
            driver: driver.to_owned(),
            drm_driver: Some(String::new()),
            sysfs_device: PathBuf::from(format!("/sys/class/drm/card{card_index}/device")),
            card_node: PathBuf::from(format!("/dev/dri/card{card_index}")),
            card_dev: (226, card_index),
            render_node: Some(PathBuf::from(format!(
                "/dev/dri/renderD{}",
                128 + card_index
            ))),
            render_dev: Some((226, 128 + card_index)),
        }
    }

    fn devices() -> Vec<DrmDevice> {
        vec![
            drm_device("0000:0a:00.0", Bus::Pci, "nvidia", 0),
            drm_device("1002000000.v3d", Bus::Platform, "v3d", 1),
        ]
    }

    #[test]
    fn matches_a_soc_gpu_by_driver_name() {
        // nvtop leaves `pdev` empty for v3d, msm, panfrost and panthor
        assert_eq!(match_device(&mut devices(), "", "v3d"), Some(1));
        assert_eq!(match_device(&mut devices(), "", "panthor"), None);
    }

    #[test]
    fn matches_a_soc_gpu_whose_drm_driver_differs_from_its_bus_driver() {
        let mut devices = devices();
        devices[1].driver = "mali".to_owned();
        devices[1].drm_driver = Some("panthor".to_owned());

        assert_eq!(match_device(&mut devices, "", "panthor"), Some(1));
        assert_eq!(match_device(&mut devices, "", "mali"), Some(1));
    }

    #[test]
    fn recognizes_pci_addresses() {
        assert!(is_pci_address("0000:0a:00.0"));
        assert!(is_pci_address("0000:0A:00.0"));
        assert!(is_pci_address("00000000:65:00.0"));
    }

    #[test]
    fn rejects_anything_that_is_not_a_pci_address() {
        for address in ["", "RK-NPU0", "TPU0", "1-2"] {
            assert!(!is_pci_address(address), "{address}");
        }

        for address in [
            "../../etc/passwd",
            "0000:0a:00.0/uevent",
            "0000:0a:00",
            "0000:0a:00.",
            "0:0a:00.0",
            "0000:g0:00.0",
            "0000.0a:00:0",
        ] {
            assert!(!is_pci_address(address), "{address}");
        }
    }

    #[test]
    fn matches_a_pci_gpu_regardless_of_case() {
        // NVML reports the bus ID with uppercase hex, sysfs uses lowercase
        assert_eq!(
            match_device(&mut devices(), "0000:0A:00.0", "NVIDIA"),
            Some(0)
        );
        assert_eq!(
            match_device(&mut devices(), "0000:0a:00.0", "NVIDIA"),
            Some(0)
        );
    }

    #[test]
    fn a_pci_gpu_is_never_matched_by_backend_name() {
        assert_eq!(match_device(&mut devices(), "0000:99:00.0", "nvidia"), None);
    }

    #[test]
    fn soc_gpus_probe_the_render_node_before_the_card_node() {
        let device = GpuDevice {
            id: "1002000000.v3d".to_owned(),
            sysfs_device: None,
            is_pci: false,
            drm: Some(drm_device("1002000000.v3d", Bus::Platform, "v3d", 1)),
        };

        assert_eq!(
            device.render_nodes(),
            vec![
                PathBuf::from("/dev/dri/renderD129"),
                PathBuf::from("/dev/dri/card1")
            ]
        );
    }

    #[test]
    fn a_gpu_without_a_drm_card_falls_back_to_the_by_path_symlink() {
        let device = GpuDevice {
            id: "0000:01:00.0".to_owned(),
            sysfs_device: None,
            is_pci: true,
            drm: None,
        };

        assert_eq!(
            device.render_nodes(),
            vec![PathBuf::from("/dev/dri/by-path/pci-0000:01:00.0-card")]
        );
    }

    #[test]
    fn matches_vulkan_devices_by_drm_node() {
        let device = GpuDevice {
            id: "1002000000.v3d".to_owned(),
            sysfs_device: None,
            is_pci: false,
            drm: Some(drm_device("1002000000.v3d", Bus::Platform, "v3d", 1)),
        };

        let version = magpie_platform::gpus::ApiVersion {
            major: 1,
            minor: 3,
            patch: Some(0),
        };

        assert!(device.is_vulkan_device(&VulkanDevice {
            ids_key: 0,
            primary_node: Some((226, 1)),
            render_node: None,
            version,
        }));
        assert!(device.is_vulkan_device(&VulkanDevice {
            ids_key: 0,
            primary_node: None,
            render_node: Some((226, 129)),
            version,
        }));
        // A software renderer reports no DRM node at all, it must not match
        assert!(!device.is_vulkan_device(&VulkanDevice {
            ids_key: 0,
            primary_node: None,
            render_node: None,
            version,
        }));
    }
}
