/* src/fan.rs
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

use magpie_platform::fan::Fan;

pub struct FanCache {
    fans: Vec<Fan>,
}

impl magpie_platform::fan::FanCache for FanCache {
    fn new() -> Self {
        Self { fans: Vec::new() }
    }

    fn refresh(&mut self) {
        self.fans = read_smc_fans();
    }

    fn cached_entries(&self) -> &[Fan] {
        &self.fans
    }
}

fn read_smc_fans() -> Vec<Fan> {
    let fan_count = smc_read_fan_count();
    let mut fans = Vec::with_capacity(fan_count as usize);
    for i in 0..fan_count {
        let actual_rpm = smc_read_fan_rpm(i, b"Ac");
        let max_rpm = smc_read_fan_rpm(i, b"Mx").max(1);
        let percent = if max_rpm > 0 {
            (actual_rpm as f32 / max_rpm as f32) * 100.0
        } else {
            0.0
        };
        fans.push(Fan {
            fan_label: Some(format!("Fan {i}")),
            temp_name: None,
            fan_index: i,
            hwmon_index: 0,
            rpm: actual_rpm,
            temp_amount: None,
            pwm_percent: Some(percent),
            max_rpm: Some(max_rpm),
        });
    }
    fans
}

fn smc_read_fan_count() -> u32 {
    smc_read_u8_key(b"FNum") as u32
}

fn smc_read_fan_rpm(index: u32, suffix: &[u8; 2]) -> u32 {
    let mut key = [0u8; 4];
    key[0] = b'F';
    key[1] = b'0' + (index.min(9) as u8);
    key[2] = suffix[0];
    key[3] = suffix[1];
    smc_read_fpe2_key(&key) as u32
}

fn smc_read_u8_key(key: &[u8]) -> u8 {
    unsafe {
        let conn = smc_open();
        if conn == 0 {
            return 0;
        }
        let val = smc_read_key_raw(conn, key);
        smc_close(conn);
        if val.data_size >= 1 {
            val.bytes[0]
        } else {
            0
        }
    }
}

fn smc_read_fpe2_key(key: &[u8]) -> f32 {
    unsafe {
        let conn = smc_open();
        if conn == 0 {
            return 0.0;
        }
        let val = smc_read_key_raw(conn, key);
        smc_close(conn);
        if val.data_size >= 2 {
            let raw = ((val.bytes[0] as u16) << 8) | (val.bytes[1] as u16);
            raw as f32 / 4.0
        } else {
            0.0
        }
    }
}

#[cfg(not(target_arch = "aarch64"))]
pub(crate) fn smc_cpu_temperature() -> Option<f32> {
    let temp = unsafe {
        let conn = smc_open();
        if conn == 0 {
            return None;
        }
        let value = smc_read_key_raw(conn, b"TC0P");
        smc_close(conn);
        if value.data_size >= 2 {
            let raw = u16::from_be_bytes([value.bytes[0], value.bytes[1]]);
            raw as f32 / 256.0
        } else {
            return None;
        }
    };
    if temp.is_finite() && temp > 0.0 && temp < 150.0 {
        Some(temp)
    } else {
        None
    }
}

#[repr(C)]
struct SmcVal {
    bytes: [u8; 32],
    data_size: u32,
}

impl Default for SmcVal {
    fn default() -> Self {
        Self {
            bytes: [0u8; 32],
            data_size: 0,
        }
    }
}

// Apple Silicon often has no AppleSMC user client; failures return empty.
// ponytail: open/close per key is slow; keep conn open across reads if fan count grows.
unsafe fn smc_open() -> u32 {
    let matching = IOServiceMatching(b"AppleSMC\0".as_ptr() as *const libc::c_char);
    if matching.is_null() {
        return 0;
    }
    let service = IOServiceGetMatchingService(kIOMainPortDefault, matching);
    if service == 0 {
        return 0;
    }
    let mut conn: u32 = 0;
    let kr = IOServiceOpen(service, mach_task_self(), 2, &mut conn);
    IOObjectRelease(service);
    if kr != 0 {
        return 0;
    }
    conn
}

unsafe fn smc_close(conn: u32) {
    let _ = IOServiceClose(conn);
}

unsafe fn smc_read_key_raw(conn: u32, key: &[u8]) -> SmcVal {
    let mut result = SmcVal::default();
    if key.len() < 4 {
        return result;
    }
    let key_u32 = u32::from_be_bytes([key[0], key[1], key[2], key[3]]);

    #[repr(C)]
    struct SmcKeyData {
        key: u32,
        vers: [u8; 6],
        p_limit_data: [u8; 16],
        key_info: SmcKeyInfo,
        result: u8,
        status: u8,
        data8: u8,
        data32: u32,
        bytes: [u8; 32],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SmcKeyInfo {
        data_size: u32,
        data_type: u32,
        data_attributes: u8,
    }

    let mut input: SmcKeyData = std::mem::zeroed();
    let mut output: SmcKeyData = std::mem::zeroed();
    input.key = key_u32;
    input.data8 = 9; // kSMCGetKeyInfo

    let input_size = std::mem::size_of::<SmcKeyData>();
    let mut output_size = input_size;

    let kr = IOConnectCallStructMethod(
        conn,
        2,
        &input as *const SmcKeyData as *const libc::c_void,
        input_size,
        &mut output as *mut SmcKeyData as *mut libc::c_void,
        &mut output_size,
    );
    if kr != 0 {
        return result;
    }

    input.key_info.data_size = output.key_info.data_size;
    input.key_info.data_type = output.key_info.data_type;
    input.data8 = 5; // kSMCReadKey

    output_size = input_size;
    let kr = IOConnectCallStructMethod(
        conn,
        2,
        &input as *const SmcKeyData as *const libc::c_void,
        input_size,
        &mut output as *mut SmcKeyData as *mut libc::c_void,
        &mut output_size,
    );
    if kr == 0 {
        result.data_size = output.key_info.data_size;
        result.bytes = output.bytes;
    }
    result
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceGetMatchingService(main_port: u32, matching: *mut libc::c_void) -> u32;
    fn IOServiceMatching(name: *const libc::c_char) -> *mut libc::c_void;
    fn IOServiceOpen(service: u32, owning_task: u32, r#type: u32, connect: *mut u32) -> i32;
    fn IOServiceClose(connect: u32) -> i32;
    fn IOObjectRelease(object: u32) -> i32;
    fn IOConnectCallStructMethod(
        connection: u32,
        selector: u32,
        input_struct: *const libc::c_void,
        input_struct_cnt: usize,
        output_struct: *mut libc::c_void,
        output_struct_cnt: *mut usize,
    ) -> i32;
    fn mach_task_self() -> u32;
    static kIOMainPortDefault: u32;
}
