/* sys_info_v2/gatherer/src/platform/macos/mod.rs
 *
 * Copyright 2024 Mission Center Contributors
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::time::{Duration, Instant};

use lazy_static::lazy_static;

#[allow(dead_code)]
pub use apps::*;
#[allow(dead_code)]
pub use cpu_info::*;
#[allow(dead_code)]
pub use disk_info::*;
#[allow(dead_code)]
pub use fan_info::*;
#[allow(dead_code)]
pub use gpu_info::*;
pub use processes::*;
#[allow(dead_code)]
pub use services::*;
pub use utilities::*;

mod apps;
mod cpu_info;
mod disk_info;
mod fan_info;
mod gpu_info;
pub mod processes;
mod services;
mod utilities;

const MIN_DELTA_REFRESH: Duration = Duration::from_millis(200);

lazy_static! {
    static ref CPU_COUNT: usize = {
        let mut count: u32 = 0;
        let mut size = std::mem::size_of::<u32>();
        unsafe {
            libc::sysctlbyname(
                b"hw.logicalcpu\0".as_ptr() as *const libc::c_char,
                &mut count as *mut u32 as *mut libc::c_void,
                &mut size,
                std::ptr::null_mut(),
                0,
            );
        }
        (count as usize).max(1)
    };
    static ref INITIAL_REFRESH_TS: Instant =
        Instant::now() - Duration::from_secs(u32::MAX as u64);
}
