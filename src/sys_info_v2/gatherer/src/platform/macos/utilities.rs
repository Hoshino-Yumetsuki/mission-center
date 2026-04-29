#![allow(dead_code)]
/* sys_info_v2/gatherer/src/platform/macos/utilities.rs
 *
 * Copyright 2024 Mission Center Contributors
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use crate::platform::utilities::PlatformUtilitiesExt;

#[derive(Default)]
pub struct MacosPlatformUtilities;

impl PlatformUtilitiesExt for MacosPlatformUtilities {
    fn on_main_app_exit(&self, mut callback: Box<dyn FnMut() + Send>) {
        std::thread::spawn(move || {
            let ppid = unsafe { libc::getppid() };
            if ppid <= 1 {
                return;
            }

            unsafe {
                let kq = libc::kqueue();
                if kq < 0 {
                    return;
                }

                let mut kev: libc::kevent = std::mem::zeroed();
                kev.ident = ppid as libc::uintptr_t;
                kev.filter = libc::EVFILT_PROC;
                kev.flags = libc::EV_ADD | libc::EV_ONESHOT;
                kev.fflags = libc::NOTE_EXIT;
                kev.data = 0;
                kev.udata = std::ptr::null_mut();

                let ret = libc::kevent(
                    kq,
                    &kev,
                    1,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null(),
                );
                if ret < 0 {
                    libc::close(kq);
                    return;
                }

                let mut out: libc::kevent = std::mem::zeroed();
                libc::kevent(
                    kq,
                    std::ptr::null(),
                    0,
                    &mut out,
                    1,
                    std::ptr::null(),
                );

                libc::close(kq);
            }

            callback();
        });
    }
}
