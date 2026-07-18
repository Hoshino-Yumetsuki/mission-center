/* src/gpus/vulkan_info.rs
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

use ash::vk;

use magpie_platform::gpus::ApiVersion;

pub struct VulkanInfo {
    _entry: ash::Entry,
    vk_instance: ash::Instance,
}

impl Drop for VulkanInfo {
    fn drop(&mut self) {
        unsafe {
            self.vk_instance.destroy_instance(None);
        }
    }
}

impl VulkanInfo {
    pub fn new() -> Option<Self> {
        let _entry = match unsafe { ash::Entry::load() } {
            Ok(e) => e,
            Err(e) => {
                log::error!(
                    "Failed to get Vulkan information: Could not load 'libvulkan.so.1'; {}",
                    e
                );
                return None;
            }
        };
        log::debug!("Loaded Vulkan library");

        let app_info = vk::ApplicationInfo {
            api_version: vk::make_api_version(0, 1, 0, 0),
            ..Default::default()
        };
        let create_info = vk::InstanceCreateInfo {
            p_application_info: &app_info,
            ..Default::default()
        };

        let instance = match unsafe { _entry.create_instance(&create_info, None) } {
            Ok(i) => i,
            Err(e) => {
                log::error!(
                    "Failed to get Vulkan information: Could not create instance; {}",
                    e
                );
                return None;
            }
        };
        log::debug!("Created Vulkan instance");

        Some(Self {
            _entry,
            vk_instance: instance,
        })
    }

    pub unsafe fn supported_vulkan_versions(
        &self,
    ) -> Option<std::collections::HashMap<u32, ApiVersion>> {
        let physical_devices = self
            .vk_instance
            .enumerate_physical_devices()
            .unwrap_or_else(|e| {
                log::warn!(
                    "Failed to get Vulkan information: No Vulkan capable devices found ({})",
                    e
                );
                vec![]
            });

        let mut supported_versions = std::collections::HashMap::new();

        for device in physical_devices {
            let properties = self.vk_instance.get_physical_device_properties(device);
            log::debug!(
                "Found Vulkan device: {:?}",
                std::ffi::CStr::from_ptr(properties.device_name.as_ptr())
            );

            let version = properties.api_version;
            let major = (version >> 22) as u16;
            let minor = ((version >> 12) & 0x3ff) as u16;
            let patch = (version & 0xfff) as u16;

            let vendor_id = properties.vendor_id & 0xffff;
            let device_id = properties.device_id & 0xffff;

            supported_versions.insert(
                (vendor_id << 16) | device_id,
                ApiVersion {
                    major: major as u32,
                    minor: minor as u32,
                    patch: Some(patch as u32),
                },
            );
        }

        Some(supported_versions)
    }
}
