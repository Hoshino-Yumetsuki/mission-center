#![allow(dead_code)]
/* sys_info_v2/gatherer/src/platform/macos/fan_info.rs
 *
 * Copyright 2024 Mission Center Contributors
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use crate::dbus_shim::{Append, Arg, ArgType, IterAppend, Signature};
use std::sync::Arc;

use crate::platform::fan_info::{FanInfoExt, FansInfoExt};

#[derive(Debug, Clone)]
pub struct MacosFanInfo {
    fan_label: Arc<str>,
    temp_name: Arc<str>,
    temp_amount: i64,
    rpm: u64,
    percent_vroomimg: f32,
    fan_index: u64,
    hwmon_index: u64,
    max_speed: u64,
}

impl Default for MacosFanInfo {
    fn default() -> Self {
        Self {
            fan_label: Arc::from(""),
            temp_name: Arc::from(""),
            temp_amount: 0,
            rpm: 0,
            percent_vroomimg: 0.0,
            fan_index: 0,
            hwmon_index: 0,
            max_speed: 0,
        }
    }
}

impl FanInfoExt for MacosFanInfo {
    fn fan_label(&self) -> &str {
        self.fan_label.as_ref()
    }
    fn temp_name(&self) -> &str {
        self.temp_name.as_ref()
    }
    fn temp_amount(&self) -> i64 {
        self.temp_amount
    }
    fn rpm(&self) -> u64 {
        self.rpm
    }
    fn percent_vroomimg(&self) -> f32 {
        self.percent_vroomimg
    }
    fn fan_index(&self) -> u64 {
        self.fan_index
    }
    fn hwmon_index(&self) -> u64 {
        self.hwmon_index
    }
    fn max_speed(&self) -> u64 {
        self.max_speed
    }
}

pub struct MacosFanInfoIter<'a>(
    pub std::iter::Map<
        std::slice::Iter<'a, MacosFanInfo>,
        fn(&'a MacosFanInfo) -> &'a MacosFanInfo,
    >,
);

impl<'a> Iterator for MacosFanInfoIter<'a> {
    type Item = &'a MacosFanInfo;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }
}

impl<'a> Clone for MacosFanInfoIter<'a> {
    fn clone(&self) -> Self {
        MacosFanInfoIter(self.0.clone())
    }
}

#[derive(Default)]
pub struct MacosFansInfo {
    fans: Vec<MacosFanInfo>,
}

impl MacosFansInfo {
    pub fn new() -> Self { Self::default() }
}

impl<'a> FansInfoExt<'a> for MacosFansInfo {
    type S = MacosFanInfo;
    type Iter = MacosFanInfoIter<'a>;

    fn refresh_cache(&mut self) {
        self.fans = read_smc_fans();
    }

    fn info(&'a self) -> Self::Iter {
        MacosFanInfoIter(self.fans.iter().map(|f| f as &MacosFanInfo))
    }
}

fn read_smc_fans() -> Vec<MacosFanInfo> {
    let fan_count = smc_read_fan_count();
    let mut fans = Vec::with_capacity(fan_count as usize);
    for i in 0..fan_count {
        let actual_rpm = smc_read_fan_rpm(i, b"Ac");
        let _min_rpm = smc_read_fan_rpm(i, b"Mn").max(1);
        let max_rpm = smc_read_fan_rpm(i, b"Mx").max(1);
        let percent = if max_rpm > 0 {
            (actual_rpm as f32 / max_rpm as f32) * 100.0
        } else {
            0.0
        };
        fans.push(MacosFanInfo {
            fan_label: Arc::from(format!("Fan {}", i).as_str()),
            temp_name: Arc::from(""),
            temp_amount: 0,
            rpm: actual_rpm,
            percent_vroomimg: percent,
            fan_index: i as u64,
            hwmon_index: 0,
            max_speed: max_rpm,
        });
    }
    fans
}

fn smc_read_fan_count() -> u32 {
    smc_read_u8_key(b"FNum\0") as u32
}

fn smc_read_fan_rpm(index: u32, suffix: &[u8]) -> u64 {
    let key = format!("F{}{}",
        index,
        std::str::from_utf8(suffix).unwrap_or("Ac")
    );
    smc_read_fpe2_key(key.as_bytes()) as u64
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

unsafe fn smc_open() -> u32 {
    let service = IOServiceGetMatchingService(
        kIOMasterPortDefault,
        IOServiceMatching(b"AppleSMC\0".as_ptr() as *const libc::c_char),
    );
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
    IOServiceClose(conn);
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
    struct SmcKeyInfo {
        data_size: u32,
        data_type: u32,
        data_attributes: u8,
    }

    let mut input: SmcKeyData = std::mem::zeroed();
    let mut output: SmcKeyData = std::mem::zeroed();
    input.key = key_u32;
    input.data8 = 9;

    let input_size = std::mem::size_of::<SmcKeyData>() as u32;
    let mut output_size = input_size;

    let kr = IOConnectCallStructMethod(
        conn,
        2,
        &input as *const SmcKeyData as *const libc::c_void,
        input_size as usize,
        &mut output as *mut SmcKeyData as *mut libc::c_void,
        &mut output_size as *mut u32 as *mut usize,
    );

    if kr != 0 {
        return result;
    }

    input.key_info.data_size = output.key_info.data_size;
    input.key_info.data_type = output.key_info.data_type;
    input.data8 = 5;

    let kr = IOConnectCallStructMethod(
        conn,
        2,
        &input as *const SmcKeyData as *const libc::c_void,
        input_size as usize,
        &mut output as *mut SmcKeyData as *mut libc::c_void,
        &mut output_size as *mut u32 as *mut usize,
    );

    if kr == 0 {
        result.data_size = output.key_info.data_size;
        result.bytes[..32].copy_from_slice(&output.bytes[..32]);
    }
    result
}

extern "C" {
    fn IOServiceGetMatchingService(master_port: u32, matching: *mut libc::c_void) -> u32;
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
    static kIOMasterPortDefault: u32;
}

impl Arg for MacosFanInfo {
    const ARG_TYPE: ArgType = ArgType::Struct;
    fn signature() -> Signature { Signature::from("") }
}
impl Append for MacosFanInfo {
    fn append_by_ref(&self, _: &mut IterAppend) {}
}

impl Arg for MacosFansInfo {
    const ARG_TYPE: ArgType = ArgType::Struct;
    fn signature() -> Signature { Signature::from("") }
}
impl Append for MacosFansInfo {
    fn append_by_ref(&self, _: &mut IterAppend) {}
}
