/* src/gpus/util.rs
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

use std::fmt::Write;
use std::io::Read;

use arrayvec::ArrayString;

use super::nvtop;

pub fn pci_id(gpu: &nvtop::GpuInfo) -> Option<ArrayString<16>> {
    let pdev = unsafe { std::ffi::CStr::from_ptr(gpu.pdev.as_ptr()) };
    let pdev = match pdev.to_str() {
        Ok(pd) => pd,
        Err(_) => {
            log::warn!("Unable to convert PCI ID to string: {:?}", pdev);
            return None;
        }
    };
    let pci_id = match ArrayString::<16>::from(pdev) {
        Ok(id) => id,
        Err(_) => {
            log::warn!("PCI ID exceeds 16 characters: {}", pdev);
            return None;
        }
    };

    Some(pci_id)
}

pub fn shared_mem_used(pci_id: &ArrayString<16>, buffer: &mut String) -> Option<u64> {
    const SYS_PATH_CAP: usize = "/sys/bus/pci/devices/".len() + 16 + "/mem_info_gtt_used".len();

    let mut path = ArrayString::<SYS_PATH_CAP>::new();
    let _ = write!(&mut path, "/sys/bus/pci/devices/{pci_id}/mem_info_gtt_used");

    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .open(path)
        .or({
            let mut pci_id_lower = *pci_id;
            for c in unsafe { pci_id_lower.as_bytes_mut() } {
                c.make_ascii_lowercase();
            }

            path.clear();
            let _ = write!(
                &mut path,
                "/sys/bus/pci/devices/{pci_id_lower}/mem_info_gtt_used"
            );

            std::fs::OpenOptions::new().read(true).open(path)
        })
        .or({
            let mut pci_id_upper = *pci_id;
            for c in unsafe { pci_id_upper.as_bytes_mut() } {
                c.make_ascii_uppercase();
            }

            path.clear();
            let _ = write!(
                &mut path,
                "/sys/bus/pci/devices/{pci_id_upper}/mem_info_gtt_used"
            );

            std::fs::OpenOptions::new().read(true).open(path)
        }) {
        Ok(f) => f,
        Err(e) => {
            log::debug!("Failed to open {}: {}", path, e);
            return None;
        }
    };

    file.read_to_string(buffer)
        .ok()
        .and_then(|_| buffer.trim().parse::<u64>().ok())
}

pub fn shared_mem_total(pci_id: &ArrayString<16>, buffer: &mut String) -> Option<u64> {
    const SYS_PATH_CAP: usize = "/sys/bus/pci/devices/".len() + 16 + "/mem_info_gtt_total".len();

    let mut path = ArrayString::<SYS_PATH_CAP>::new();
    let _ = write!(
        &mut path,
        "/sys/bus/pci/devices/{pci_id}/mem_info_gtt_total"
    );

    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .open(path)
        .or({
            let mut pci_id_lower = *pci_id;
            for c in unsafe { pci_id_lower.as_bytes_mut() } {
                c.make_ascii_lowercase();
            }

            path.clear();
            let _ = write!(
                &mut path,
                "/sys/bus/pci/devices/{pci_id_lower}/mem_info_gtt_total"
            );

            std::fs::OpenOptions::new().read(true).open(path)
        })
        .or({
            let mut pci_id_upper = *pci_id;
            for c in unsafe { pci_id_upper.as_bytes_mut() } {
                c.make_ascii_uppercase();
            }

            path.clear();
            let _ = write!(
                &mut path,
                "/sys/bus/pci/devices/{pci_id_upper}/mem_info_gtt_total"
            );

            std::fs::OpenOptions::new().read(true).open(path)
        }) {
        Ok(f) => f,
        Err(e) => {
            log::debug!("Failed to open {}: {}", path, e);
            return None;
        }
    };

    file.read_to_string(buffer)
        .ok()
        .and_then(|_| buffer.trim().parse::<u64>().ok())
}

pub fn pcie_speed(pci_id: &ArrayString<16>, buffer: &mut String) -> Option<u32> {
    const SYS_PATH_CAP: usize = "/sys/bus/pci/devices/".len() + 16 + "/current_link_speed".len();

    let mut path = ArrayString::<SYS_PATH_CAP>::new();
    let _ = write!(
        &mut path,
        "/sys/bus/pci/devices/{pci_id}/current_link_speed"
    );

    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .open(path)
        .or({
            let mut pci_id_lower = *pci_id;
            for c in unsafe { pci_id_lower.as_bytes_mut() } {
                c.make_ascii_lowercase();
            }

            path.clear();
            let _ = write!(
                &mut path,
                "/sys/bus/pci/devices/{pci_id_lower}/current_link_speed"
            );

            std::fs::OpenOptions::new().read(true).open(path)
        })
        .or({
            let mut pci_id_upper = *pci_id;
            for c in unsafe { pci_id_upper.as_bytes_mut() } {
                c.make_ascii_uppercase();
            }

            path.clear();
            let _ = write!(
                &mut path,
                "/sys/bus/pci/devices/{pci_id_upper}/current_link_speed"
            );

            std::fs::OpenOptions::new().read(true).open(path)
        }) {
        Ok(f) => f,
        Err(e) => {
            log::debug!("Failed to open {}: {}", path, e);
            return None;
        }
    };

    file.read_to_string(buffer).ok().and_then(|_| {
        let content = buffer.trim();
        if content.ends_with(" GT/s PCIe") {
            let last_line = content.lines().last().unwrap();
            let number = last_line.split_whitespace().next().unwrap();
            if let Some(number) = Some(number) {
                match number {
                    "2.5" => Some(1),
                    "5.0" => Some(2),
                    "8.0" => Some(3),
                    "16.0" => Some(4),
                    "32.0" => Some(5),
                    "64.0" => Some(6),
                    _ => None,
                }
            } else {
                None
            }
        } else {
            None
        }
    })
}

pub fn max_pcie_speed(pci_id: &ArrayString<16>, buffer: &mut String) -> Option<u32> {
    const SYS_PATH_CAP: usize = "/sys/bus/pci/devices/".len() + 16 + "/max_link_speed".len();

    let mut path = ArrayString::<SYS_PATH_CAP>::new();
    let _ = write!(&mut path, "/sys/bus/pci/devices/{pci_id}/max_link_speed");

    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .open(path)
        .or({
            let mut pci_id_lower = *pci_id;
            for c in unsafe { pci_id_lower.as_bytes_mut() } {
                c.make_ascii_lowercase();
            }

            path.clear();
            let _ = write!(
                &mut path,
                "/sys/bus/pci/devices/{pci_id_lower}/max_link_speed"
            );

            std::fs::OpenOptions::new().read(true).open(path)
        })
        .or({
            let mut pci_id_upper = *pci_id;
            for c in unsafe { pci_id_upper.as_bytes_mut() } {
                c.make_ascii_uppercase();
            }

            path.clear();
            let _ = write!(
                &mut path,
                "/sys/bus/pci/devices/{pci_id_upper}/max_link_speed"
            );

            std::fs::OpenOptions::new().read(true).open(path)
        }) {
        Ok(f) => f,
        Err(e) => {
            log::debug!("Failed to open {}: {}", path, e);
            return None;
        }
    };

    file.read_to_string(buffer).ok().and_then(|_| {
        let content = buffer.trim();
        if content.ends_with(" GT/s PCIe") {
            let last_line = content.lines().last().unwrap();
            let number = last_line.split_whitespace().next().unwrap();
            if let Some(number) = Some(number) {
                match number {
                    "2.5" => Some(1),
                    "5.0" => Some(2),
                    "8.0" => Some(3),
                    "16.0" => Some(4),
                    "32.0" => Some(5),
                    "64.0" => Some(6),
                    _ => None,
                }
            } else {
                None
            }
        } else {
            None
        }
    })
}

pub fn pcie_width(pci_id: &ArrayString<16>, buffer: &mut String) -> Option<u32> {
    const SYS_PATH_CAP: usize = "/sys/bus/pci/devices/".len() + 16 + "/current_link_width".len();

    let mut path = ArrayString::<SYS_PATH_CAP>::new();
    let _ = write!(
        &mut path,
        "/sys/bus/pci/devices/{pci_id}/current_link_width"
    );

    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .open(path)
        .or({
            let mut pci_id_lower = *pci_id;
            for c in unsafe { pci_id_lower.as_bytes_mut() } {
                c.make_ascii_lowercase();
            }

            path.clear();
            let _ = write!(
                &mut path,
                "/sys/bus/pci/devices/{pci_id_lower}/current_link_width"
            );

            std::fs::OpenOptions::new().read(true).open(path)
        })
        .or({
            let mut pci_id_upper = *pci_id;
            for c in unsafe { pci_id_upper.as_bytes_mut() } {
                c.make_ascii_uppercase();
            }

            path.clear();
            let _ = write!(
                &mut path,
                "/sys/bus/pci/devices/{pci_id_upper}/current_link_width"
            );

            std::fs::OpenOptions::new().read(true).open(path)
        }) {
        Ok(f) => f,
        Err(e) => {
            log::debug!("Failed to open {}: {}", path, e);
            return None;
        }
    };

    file.read_to_string(buffer)
        .ok()
        .and_then(|_| buffer.lines().last().unwrap().trim().parse::<u32>().ok())
}

pub fn max_pcie_width(pci_id: &ArrayString<16>, buffer: &mut String) -> Option<u32> {
    const SYS_PATH_CAP: usize = "/sys/bus/pci/devices/".len() + 16 + "/max_link_width".len();

    let mut path = ArrayString::<SYS_PATH_CAP>::new();
    let _ = write!(&mut path, "/sys/bus/pci/devices/{pci_id}/max_link_width");

    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .open(path)
        .or({
            let mut pci_id_lower = *pci_id;
            for c in unsafe { pci_id_lower.as_bytes_mut() } {
                c.make_ascii_lowercase();
            }

            path.clear();
            let _ = write!(
                &mut path,
                "/sys/bus/pci/devices/{pci_id_lower}/max_link_width"
            );

            std::fs::OpenOptions::new().read(true).open(path)
        })
        .or({
            let mut pci_id_upper = *pci_id;
            for c in unsafe { pci_id_upper.as_bytes_mut() } {
                c.make_ascii_uppercase();
            }

            path.clear();
            let _ = write!(
                &mut path,
                "/sys/bus/pci/devices/{pci_id_upper}/max_link_width"
            );

            std::fs::OpenOptions::new().read(true).open(path)
        }) {
        Ok(f) => f,
        Err(e) => {
            log::debug!("Failed to open {}: {}", path, e);
            return None;
        }
    };

    file.read_to_string(buffer)
        .ok()
        .and_then(|_| buffer.lines().last().unwrap().trim().parse::<u32>().ok())
}

pub fn ven_dev_ids(pci_id: &ArrayString<16>, buffer: &mut String) -> Option<(u32, u32)> {
    const SYS_PATH_CAP: usize = "/sys/bus/pci/devices/".len() + 16 + "/uevent".len();

    let mut path = ArrayString::<SYS_PATH_CAP>::new();
    let _ = write!(&mut path, "/sys/bus/pci/devices/{pci_id}/uevent");
    let mut uevent_file = match std::fs::OpenOptions::new()
        .read(true)
        .open(path)
        .or({
            let mut pci_id_lower = *pci_id;
            for c in unsafe { pci_id_lower.as_bytes_mut() } {
                c.make_ascii_lowercase();
            }

            path.clear();
            let _ = write!(&mut path, "/sys/bus/pci/devices/{pci_id_lower}/uevent");

            std::fs::OpenOptions::new().read(true).open(path)
        })
        .or({
            let mut pci_id_upper = *pci_id;
            for c in unsafe { pci_id_upper.as_bytes_mut() } {
                c.make_ascii_uppercase();
            }

            path.clear();
            let _ = write!(&mut path, "/sys/bus/pci/devices/{pci_id_upper}/uevent");

            std::fs::OpenOptions::new().read(true).open(path)
        }) {
        Ok(f) => f,
        Err(e) => {
            log::error!("Unable to open `{path}` file for device `{pci_id}`: {e}");
            return None;
        }
    };

    buffer.clear();
    match uevent_file.read_to_string(buffer) {
        Ok(_) => {
            let mut vendor_id = None;
            let mut device_id = None;

            for line in buffer.lines().map(|l| l.trim()) {
                if let Some(line) = line.strip_prefix("PCI_ID=") {
                    let mut ids = line.split(':');
                    vendor_id = ids
                        .next()
                        .and_then(|id| u16::from_str_radix(id, 16).ok())
                        .map(|id| id as u32);
                    device_id = ids
                        .next()
                        .and_then(|id| u16::from_str_radix(id, 16).ok())
                        .map(|id| id as u32);
                    break;
                }
            }

            match (vendor_id, device_id) {
                (Some(v), Some(d)) => Some((v, d)),
                _ => {
                    log::error!(
                        "Unable to parse vendor and device IDs from `uevent` file content for device {pci_id}",
                    );
                    None
                }
            }
        }
        Err(_) => {
            log::error!("Unable to read `uevent` file content for device {pci_id}");
            None
        }
    }
}
