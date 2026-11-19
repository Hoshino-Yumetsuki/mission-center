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
    let Some(count) = smc_read_key(b"FNum") else {
        return Vec::new();
    };
    if count.data_type != *b"ui8 " || count.data_size != 1 {
        return Vec::new();
    }
    // SMC fan keys have one hexadecimal index digit.
    (0..u32::from(count.bytes[0]).min(16))
        .filter_map(|i| {
            let rpm = smc_read_fan_rpm(i, b"Ac")?;
            Some(Fan {
                fan_label: Some(format!("Fan {i}")),
                temp_name: None,
                fan_index: i,
                hwmon_index: 0,
                rpm,
                temp_amount: None,
                // RPM/max RPM is not PWM duty cycle.
                pwm_percent: None,
                max_rpm: smc_read_fan_rpm(i, b"Mx").filter(|&rpm| rpm > 0),
            })
        })
        .collect()
}

fn smc_read_fan_rpm(index: u32, suffix: &[u8; 2]) -> Option<u32> {
    let key = [
        b'F',
        b"0123456789ABCDEF"[index as usize],
        suffix[0],
        suffix[1],
    ];
    let value = smc_read_key(&key)?.number()?;
    (value.is_finite() && (0.0..=u32::MAX as f32).contains(&value)).then_some(value as u32)
}

fn smc_read_key(key: &[u8; 4]) -> Option<SmcVal> {
    unsafe {
        let conn = smc_open();
        if conn == 0 {
            return None;
        }
        let value = smc_read_key_raw(conn, key);
        smc_close(conn);
        value
    }
}

#[cfg(not(target_arch = "aarch64"))]
pub(crate) fn smc_cpu_temperature() -> Option<f32> {
    let value = smc_read_key(b"TC0P")?;
    if value.data_type != *b"sp78" || value.data_size != 2 {
        return None;
    }
    let temp = i16::from_be_bytes([value.bytes[0], value.bytes[1]]) as f32 / 256.0;
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
    data_type: [u8; 4],
}

impl SmcVal {
    fn number(&self) -> Option<f32> {
        match (&self.data_type, self.data_size) {
            (b"fpe2", 2) => Some(u16::from_be_bytes([self.bytes[0], self.bytes[1]]) as f32 / 4.0),
            (b"flt ", 4) => Some(f32::from_le_bytes(self.bytes[..4].try_into().ok()?)),
            _ => None,
        }
    }
}

impl Default for SmcVal {
    fn default() -> Self {
        Self {
            bytes: [0u8; 32],
            data_size: 0,
            data_type: [0; 4],
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

unsafe fn smc_read_key_raw(conn: u32, key: &[u8; 4]) -> Option<SmcVal> {
    let key_u32 = u32::from_be_bytes(*key);

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
    if kr != 0 || output.result != 0 || output_size != input_size || output.key_info.data_size > 32
    {
        return None;
    }

    input.key_info.data_size = output.key_info.data_size;
    let data_type = output.key_info.data_type.to_be_bytes();
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
    if kr != 0 || output.result != 0 || output_size != input_size {
        return None;
    }
    Some(SmcVal {
        data_size: input.key_info.data_size,
        data_type,
        bytes: output.bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::{read_smc_fans, SmcVal};

    #[test]
    fn decodes_smc_fan_values() {
        let mut value = SmcVal::default();
        value.data_type = *b"fpe2";
        value.data_size = 2;
        value.bytes[..2].copy_from_slice(&8000u16.to_be_bytes());
        assert_eq!(value.number(), Some(2000.0));
        value.data_type = *b"flt ";
        value.data_size = 4;
        value.bytes[..4].copy_from_slice(&2345.5f32.to_le_bytes());
        assert_eq!(value.number(), Some(2345.5));
        value.data_size = 2;
        assert_eq!(value.number(), None);
    }

    #[test]
    fn native_fan_readings_are_optional() {
        let fans = read_smc_fans();
        for fan in &fans {
            assert_eq!(fan.pwm_percent, None);
            assert_ne!(fan.max_rpm, Some(0));
            eprintln!(
                "fan {}: {} RPM, max {:?} RPM",
                fan.fan_index, fan.rpm, fan.max_rpm
            );
        }
        eprintln!("read {} fans", fans.len());
    }
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
