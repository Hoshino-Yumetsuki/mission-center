/* src/main.rs
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

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use clap::Parser;
use log::LevelFilter;
use prost::Message;
use signal_hook::consts::signal::*;
use signal_hook::flag as signal_flag;

use magpie_platform::ipc::{Request, RequestBody, Response, ResponseBody, ResponseError};
use magpie_platform::DataCache;

#[cfg(target_os = "linux")]
use magpie_platform_linux as magpie_platform_os;

#[cfg(target_os = "macos")]
use magpie_platform_macos as magpie_platform_os;

use magpie_platform_os::DataCacheTypes;

mod about;
mod apps;
mod battery;
mod cpu;
mod disks;
mod fan;
mod gpus;
mod memory;
mod network;
mod nng;
mod processes;
mod services;
mod setup_script;

fn data_cache() -> &'static DataCache<DataCacheTypes> {
    static DATA_CACHE: OnceLock<DataCache<DataCacheTypes>> = OnceLock::new();
    DATA_CACHE.get_or_init(DataCache::new)
}

fn os_signals_listen(flag: Arc<AtomicBool>) {
    const SIGTERM_U: usize = SIGTERM as usize;
    const SIGINT_U: usize = SIGINT as usize;
    const SIGQUIT_U: usize = SIGQUIT as usize;

    let term = Arc::new(AtomicUsize::new(0));

    signal_flag::register_usize(SIGTERM, Arc::clone(&term), SIGTERM_U)
        .expect("Failed to register TERM signal");
    signal_flag::register_usize(SIGINT, Arc::clone(&term), SIGINT_U)
        .expect("Failed to register INT signal");
    signal_flag::register_usize(SIGQUIT, Arc::clone(&term), SIGQUIT_U)
        .expect("Failed to register QUIT signal");

    loop {
        match term.load(Ordering::Relaxed) {
            0 => {
                std::thread::sleep(Duration::from_millis(50));
            }
            SIGTERM_U => {
                log::debug!("Caught TERM signal, stopping...");
                flag.store(true, Ordering::Relaxed);
                break;
            }
            SIGINT_U => {
                log::debug!("Caught INT signal, stopping...");
                flag.store(true, Ordering::Relaxed);
                break;
            }
            SIGQUIT_U => {
                log::debug!("Caught QUIT signal, stopping...");
                flag.store(true, Ordering::Relaxed);
                break;
            }
            _ => unreachable!(),
        }
    }
}

fn handle_request(request_data: &[u8], socket: nng::nng_socket) {
    let req = match Request::decode(request_data) {
        Ok(req) => req,
        Err(e) => {
            log::error!("{:?}", e);
            return;
        }
    };

    let response = if let Some(body) = req.body {
        match body {
            RequestBody::GetAbout(_) => about::handle_request(),
            RequestBody::GetApps(_) => apps::handle_request(),
            RequestBody::GetAppsIcons(req) => apps::handle_icons_request(req.app_ids),
            RequestBody::GetBattery(_) => battery::handle_request(),
            RequestBody::GetCpu(_) => cpu::handle_request(),
            RequestBody::GetDisks(req) => disks::handle_request(req.request),
            RequestBody::GetFans(_) => fan::handle_request(),
            RequestBody::GetGpus(_) => gpus::handle_request(),
            RequestBody::GetMemory(req) => memory::handle_request(req.kind()),
            RequestBody::GetConnections(_) => network::handle_request(),
            RequestBody::GetProcesses(req) => processes::handle_request(req.request),
            RequestBody::GetServices(req) => services::handle_request(req.request),
            RequestBody::GetScript(req) => setup_script::handle_request(req.request),
        }
    } else {
        log::error!("Empty request");

        let response = Response {
            body: Some(ResponseBody::Error(ResponseError {
                message: "Empty request".to_string(),
            })),
        };

        let mut buffer = nng::Buffer::new(response.encoded_len());
        response.encode_raw(&mut buffer);

        buffer
    };

    let (response, response_len) = response.leak();
    let res = unsafe {
        nng::nng_send(
            socket,
            response as *mut _,
            response_len,
            nng::NNG_FLAG_ALLOC,
        )
    };
    match res {
        nng::NNG_OK => {}
        nng::NNG_EAGAIN => {
            log::error!("Failed to send reply: The operation would block, but NNG_FLAG_NONBLOCK was specified");
        }
        nng::NNG_ECLOSED => {
            log::error!("Failed to send reply: The socket is not open");
        }
        nng::NNG_EINVAL => {
            log::error!("Failed to send reply: An invalid set of flags was specified");
        }
        nng::NNG_EMSGSIZE => {
            log::error!("Failed to send reply: The value of size is too large");
        }
        nng::NNG_ENOMEM => {
            log::error!("Failed to send reply: Insufficient memory is available");
        }
        nng::NNG_ENOTSUP => {
            log::error!("Failed to send reply: The protocol for socket does not support sending");
        }
        nng::NNG_ESTATE => {
            log::error!("Failed to send reply: The socket cannot send data in this state");
        }
        nng::NNG_ETIMEDOUT => {
            log::error!("Failed to send reply: The operation timed out");
        }
        _ => {
            log::error!("Failed to send reply: Unknown error: {}", res);
        }
    }
}

fn listen(socket_addr: &str, stop_requested: Arc<AtomicBool>) -> magpie_platform::Result<()> {
    let mut socket = nng::nng_socket { id: 0 };
    let res = unsafe { nng::nng_rep0_open(&mut socket) };
    match res {
        nng::NNG_OK => {}
        nng::NNG_ENOMEM => {
            log::error!("Failed to open socket: Out of memory");
            return Err(magpie_platform::Error::msg("Failed to open socket"));
        }
        nng::NNG_ENOTSUP => {
            log::error!("Failed to open socket: Protocol not supported");
            return Err(magpie_platform::Error::msg("Failed to open socket"));
        }
        _ => {
            log::error!("Failed to open socket: Unknown error: {}", res);
            return Err(magpie_platform::Error::msg("Failed to open socket"));
        }
    }

    let res = unsafe { nng::nng_setopt_ms(socket, nng::NNG_OPT_RECVTIMEO.as_ptr() as _, 100) };
    match res {
        nng::NNG_OK => {}
        nng::NNG_ECLOSED => {
            log::error!(
                "Failed to configure socket timeout: Parameter does not refer to an open socket"
            );
            return Err(magpie_platform::Error::msg(
                "Failed to configure socket timeout",
            ));
        }
        nng::NNG_EINVAL => {
            log::error!("Failed to configure socket timeout: The value being passed is invalid");
            return Err(magpie_platform::Error::msg(
                "Failed to configure socket timeout",
            ));
        }
        nng::NNG_ENOTSUP => {
            log::error!("Failed to configure socket timeout: The option opt is not supported");
            return Err(magpie_platform::Error::msg(
                "Failed to configure socket timeout",
            ));
        }
        nng::NNG_EREADONLY => {
            log::error!("Failed to configure socket timeout: The option opt is read-only");
            return Err(magpie_platform::Error::msg(
                "Failed to configure socket timeout",
            ));
        }
        nng::NNG_ESTATE => {
            log::error!("Failed to configure socket timeout: The socket is in an inappropriate state for setting this option");
            return Err(magpie_platform::Error::msg(
                "Failed to configure socket timeout",
            ));
        }
        _ => {
            log::error!("Failed to configure socket timeout: Unknown error: {}", res);
            return Err(magpie_platform::Error::msg(
                "Failed to configure socket timeout",
            ));
        }
    }

    let res = unsafe {
        nng::nng_listen(
            socket,
            socket_addr.as_ptr() as *const _,
            std::ptr::null_mut(),
            0,
        )
    };
    match res {
        nng::NNG_OK => {}
        nng::NNG_EADDRINUSE => {
            log::error!("Failed to listen on {socket_addr}: Address in use");
            return Err(magpie_platform::Error::msg("Failed to listen"));
        }
        nng::NNG_EADDRINVAL => {
            log::error!("Failed to listen on {socket_addr}: Address invalid");
            return Err(magpie_platform::Error::msg("Failed to listen"));
        }
        nng::NNG_ECLOSED => {
            log::error!("Failed to listen on {socket_addr}: Connection closed prematurely");
            return Err(magpie_platform::Error::msg("Failed to listen"));
        }
        nng::NNG_EINVAL => {
            log::error!("Failed to listen on {socket_addr}: Invalid argument");
            return Err(magpie_platform::Error::msg("Failed to listen"));
        }
        nng::NNG_ENOMEM => {
            log::error!("Failed to listen on {socket_addr}: Out of memory");
            return Err(magpie_platform::Error::msg("Failed to listen"));
        }
        _ => {
            log::error!("Failed to listen on {socket_addr}: Unknown error: {res}");
            return Err(magpie_platform::Error::msg("Failed to listen"));
        }
    }

    log::info!("Listening on {}...", socket_addr);

    std::thread::spawn({
        let stop_requested = stop_requested.clone();
        move || os_signals_listen(stop_requested)
    });

    while !stop_requested.load(Ordering::Relaxed) {
        let mut message_buffer: *mut libc::c_void = std::ptr::null_mut();
        let mut message_len: libc::size_t = 0;

        let res = unsafe {
            nng::nng_recv(
                socket,
                (&mut message_buffer) as *mut *mut _ as *mut _,
                &mut message_len,
                nng::NNG_FLAG_ALLOC,
            )
        };
        match res {
            nng::NNG_OK => {}
            nng::NNG_ETIMEDOUT => {
                log::debug!("No message received for 100ms, waiting and trying again...");
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            nng::NNG_EAGAIN => {
                log::error!("Failed to read message: The operation would block, but NNG_FLAG_NONBLOCK was specified");
                continue;
            }
            nng::NNG_ECLOSED => {
                log::error!("Failed to read message: The socket is not open");
                continue;
            }
            nng::NNG_EINVAL => {
                log::error!("Failed to read message: An invalid set of flags was specified");
                continue;
            }
            nng::NNG_EMSGSIZE => {
                log::error!(
                    "Failed to read message: The received message did not fit in the size provided"
                );
                continue;
            }
            nng::NNG_ENOMEM => {
                log::error!("Failed to read message: Insufficient memory is available");
                continue;
            }
            nng::NNG_ENOTSUP => {
                log::error!(
                    "Failed to read message: The protocol for socket does not support receiving"
                );
                continue;
            }
            nng::NNG_ESTATE => {
                log::error!("Failed to read message: The socket cannot receive data in this state");
                continue;
            }
            _ => {
                log::error!("Failed to read message: Unknown error: {}", res);
                continue;
            }
        }

        if message_len == 0 || message_buffer.is_null() {
            log::error!("Failed to read message: Empty message");
            continue;
        }

        let request =
            unsafe { core::slice::from_raw_parts(message_buffer as *const u8, message_len) };
        handle_request(request, socket);
        unsafe { nng::nng_free(message_buffer, message_len) };
    }

    Ok(())
}

fn main() -> magpie_platform::Result<()> {
    #[derive(Parser, Debug)]
    #[command(version, about, long_about = None)]
    struct Args {
        #[arg(long)]
        addr: Option<String>,

        #[arg(long)]
        test: bool,
    }

    let mut args = Args::parse();
    if args.test {
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
    }

    env_logger::init();
    unsafe { nng::nng_log_set_logger(Some(nng::nng_log_fn)) };
    match log::max_level() {
        LevelFilter::Off => {
            unsafe { nng::nng_log_set_level(nng::NNG_LOG_NONE) };
        }
        LevelFilter::Error => {
            unsafe { nng::nng_log_set_level(nng::NNG_LOG_ERR) };
        }
        LevelFilter::Warn => {
            unsafe { nng::nng_log_set_level(nng::NNG_LOG_WARN) };
        }
        LevelFilter::Info => {
            unsafe { nng::nng_log_set_level(nng::NNG_LOG_INFO) };
        }
        LevelFilter::Debug => {
            unsafe { nng::nng_log_set_level(nng::NNG_LOG_DEBUG) };
        }
        LevelFilter::Trace => {
            unsafe { nng::nng_log_set_level(nng::NNG_LOG_DEBUG) };
        }
    }

    let addr = match std::mem::take(&mut args.addr) {
        Some(mut addr) => {
            addr.push('\0');
            addr
        }
        None => {
            return Err(magpie_platform::Error::msg(
                "No address provided, use `--addr` flag",
            ));
        }
    };

    // Kick off initial refresh in the background so the IPC socket can listen
    // immediately. macOS collectors (and cold Linux starts) can take >1s; the
    // UI client only retries ~15s and would otherwise fail to dial.
    {
        let data_cache = data_cache();
        data_cache.refresh_async();
    }

    let stop_requested = Arc::new(AtomicBool::new(false));
    listen(&addr, stop_requested)
}
