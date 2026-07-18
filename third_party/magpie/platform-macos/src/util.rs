/* src/util.rs
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

use std::ffi::CString;

pub fn sysctl_string(name: &str) -> Option<String> {
    unsafe {
        let cname = CString::new(name).ok()?;
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
        let mut buf = vec![0u8; size];
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

pub fn sysctl_u32(name: &str) -> Option<u32> {
    unsafe {
        let cname = CString::new(name).ok()?;
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

pub fn sysctl_u64(name: &str) -> Option<u64> {
    unsafe {
        let cname = CString::new(name).ok()?;
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

pub fn uptime_seconds() -> u64 {
    unsafe {
        let mut tv: libc::timeval = std::mem::zeroed();
        let mut size = std::mem::size_of::<libc::timeval>();
        let mut mib = [libc::CTL_KERN, libc::KERN_BOOTTIME];
        if libc::sysctl(
            mib.as_mut_ptr(),
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
