/* sys_info_v2/gatherer/src/main.rs
 *
 * Copyright 2024 Romeo Calota
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

use std::sync::{
    atomic::{self, AtomicBool, AtomicU64},
    Arc, PoisonError, RwLock,
};
#[cfg(target_os = "linux")]
use std::sync::Mutex;

#[cfg(target_os = "linux")]
use dbus::arg::RefArg;
#[cfg(target_os = "linux")]
use dbus::{arg, blocking::SyncConnection, channel::MatchingReceiver};
#[cfg(target_os = "linux")]
use dbus_crossroads::Crossroads;
use lazy_static::lazy_static;

#[cfg(target_os = "linux")]
use crate::platform::{FanInfo, FansInfo, FansInfoExt};
#[cfg(target_os = "macos")]
use crate::platform::FansInfoExt;
#[cfg(target_os = "macos")]
use crate::platform::FansInfo;
use logging::{debug, error, message, warning};
#[cfg(target_os = "linux")]
use logging::critical;
#[cfg(target_os = "linux")]
use platform::{
    CpuDynamicInfo, CpuStaticInfo, DiskInfo, GpuDynamicInfo, GpuStaticInfo,
    Service, ServiceControllerExt, ServicesError,
};
use platform::{
    Apps, AppsExt, CpuInfo, CpuInfoExt, CpuStaticInfoExt, DisksInfo, DisksInfoExt,
    GpuInfo, GpuInfoExt, PlatformUtilitiesExt, Processes, ProcessesExt,
    ServiceController, Services, ServicesExt,
};

#[allow(unused_imports)]
mod logging;
mod platform;
mod utils;
mod dbus_shim;

#[cfg(target_os = "linux")]
const DBUS_OBJECT_PATH: &str = "/io/missioncenter/MissionCenter/Gatherer";

lazy_static! {
    static ref SYSTEM_STATE: SystemState<'static> = {
        let system_state = SystemState::new();

        let service_controller = system_state
            .services
            .read()
            .unwrap()
            .controller()
            .map(|sc| Some(sc))
            .unwrap_or_else(|e| {
                error!(
                    "Gatherer::Main",
                    "Failed to create service controller: {}", e
                );
                None
            });

        *system_state.service_controller.write().unwrap() = service_controller;

        system_state
            .cpu_info
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .refresh_static_info_cache();

        system_state.gpu_info.write().unwrap().refresh_gpu_list();
        system_state
            .gpu_info
            .write()
            .unwrap()
            .refresh_static_info_cache();

        system_state.snapshot();

        system_state
    };
    static ref LOGICAL_CPU_COUNT: u32 = {
        SYSTEM_STATE
            .cpu_info
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .static_info()
            .logical_cpu_count()
    };
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
pub struct OrgFreedesktopDBusNameLost {
    pub arg0: String,
}

#[cfg(target_os = "linux")]
impl arg::AppendAll for OrgFreedesktopDBusNameLost {
    fn append(&self, i: &mut arg::IterAppend) {
        arg::RefArg::append(&self.arg0, i);
    }
}

#[cfg(target_os = "linux")]
impl arg::ReadAll for OrgFreedesktopDBusNameLost {
    fn read(i: &mut arg::Iter) -> Result<Self, arg::TypeMismatchError> {
        Ok(OrgFreedesktopDBusNameLost { arg0: i.read()? })
    }
}

#[cfg(target_os = "linux")]
impl dbus::message::SignalArgs for OrgFreedesktopDBusNameLost {
    const NAME: &'static str = "NameLost";
    const INTERFACE: &'static str = "org.freedesktop.DBus";
}

struct SystemState<'a> {
    cpu_info: Arc<RwLock<CpuInfo>>,
    disk_info: Arc<RwLock<DisksInfo>>,
    gpu_info: Arc<RwLock<GpuInfo>>,
    fan_info: Arc<RwLock<FansInfo>>,
    services: Arc<RwLock<Services<'a>>>,
    service_controller: Arc<RwLock<Option<ServiceController<'a>>>>,
    processes: Arc<RwLock<Processes>>,
    apps: Arc<RwLock<Apps>>,

    refresh_interval: Arc<AtomicU64>,
    core_count_affects_percentages: Arc<AtomicBool>,
}

impl SystemState<'_> {
    pub fn snapshot(&self) {
        {
            let mut processes = self
                .processes
                .write()
                .unwrap_or_else(PoisonError::into_inner);

            let timer = std::time::Instant::now();
            processes.refresh_cache();
            if !self
                .core_count_affects_percentages
                .load(atomic::Ordering::Relaxed)
            {
                let logical_cpu_count = *LOGICAL_CPU_COUNT as f32;
                for (_, p) in processes.process_list_mut() {
                    p.usage_stats.cpu_usage /= logical_cpu_count;
                }
            }
            debug!(
                "Gatherer::Perf",
                "Refreshed process cache in {:?}",
                timer.elapsed()
            );
        }

        let timer = std::time::Instant::now();
        self.cpu_info
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .refresh_dynamic_info_cache(
                &self
                    .processes
                    .read()
                    .unwrap_or_else(PoisonError::into_inner),
            );
        debug!(
            "Gatherer::Perf",
            "Refreshed CPU dynamic info cache in {:?}",
            timer.elapsed()
        );

        let timer = std::time::Instant::now();
        self.disk_info
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .refresh_cache();
        debug!(
            "Gatherer::Perf",
            "Refreshed disk info cache in {:?}",
            timer.elapsed()
        );

        let timer = std::time::Instant::now();
        self.gpu_info
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .refresh_dynamic_info_cache(
                &mut self
                    .processes
                    .write()
                    .unwrap_or_else(PoisonError::into_inner),
            );
        debug!(
            "Gatherer::Perf",
            "Refreshed GPU dynamic info cache in {:?}",
            timer.elapsed()
        );

        let timer = std::time::Instant::now();
        self.fan_info
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .refresh_cache();
        debug!(
            "Gatherer::Perf",
            "Refreshed fan info cache in {:?}",
            timer.elapsed()
        );

        let timer = std::time::Instant::now();
        self.apps
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .refresh_cache(
                self.processes
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .process_list(),
            );
        debug!(
            "Gatherer::Perf",
            "Refreshed app cache in {:?}",
            timer.elapsed()
        );

        let timer = std::time::Instant::now();
        self.services
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .refresh_cache()
            .unwrap_or_else(|e| {
                debug!("Gatherer::Main", "Failed to refresh service cache: {}", e);
            });
        debug!(
            "Gatherer::Perf",
            "Refreshed service cache in {:?}",
            timer.elapsed()
        );
    }
}

impl<'a> SystemState<'a> {
    pub fn new() -> Self {
        Self {
            cpu_info: Arc::new(RwLock::new(CpuInfo::new())),
            disk_info: Arc::new(RwLock::new(DisksInfo::new())),
            gpu_info: Arc::new(RwLock::new(GpuInfo::new())),
            fan_info: Arc::new(RwLock::new(FansInfo::new())),
            services: Arc::new(RwLock::new(Services::new())),
            service_controller: Arc::new(RwLock::new(None)),
            processes: Arc::new(RwLock::new(Processes::new())),
            apps: Arc::new(RwLock::new(Apps::new())),

            refresh_interval: Arc::new(AtomicU64::new(1000)),
            core_count_affects_percentages: Arc::new(AtomicBool::new(true)),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Exit if any arguments are passed to this executable. This is done since the main app needs
    // to check if the executable can be run in its current environment (glibc or musl libc)
    for (i, _) in std::env::args().enumerate() {
        if i > 0 {
            eprintln!("👋");
            std::process::exit(0);
        }
    }

    #[cfg(target_os = "linux")]
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
    }

    message!(
        "Gatherer::Main",
        "Starting v{}...",
        env!("CARGO_PKG_VERSION")
    );

    message!("Gatherer::Main", "Initializing system state...");
    let _ = &*SYSTEM_STATE;
    let _ = &*LOGICAL_CPU_COUNT;

    message!(
        "Gatherer::Main",
        "Setting up background data refresh thread..."
    );
    std::thread::spawn({
        move || loop {
            let refresh_interval = SYSTEM_STATE
                .refresh_interval
                .load(atomic::Ordering::Relaxed);
            std::thread::sleep(std::time::Duration::from_millis(refresh_interval));

            SYSTEM_STATE.snapshot();
        }
    });

    message!("Gatherer::Main", "Initializing platform utilities...");
    let plat_utils = platform::PlatformUtilities::default();

    message!("Gatherer::Main", "Setting up connection to main app...");
    // Set up so that the Gatherer exists when the main app exits
    plat_utils.on_main_app_exit(Box::new(|| {
        message!("Gatherer::Main", "Parent process exited, exiting...");
        std::process::exit(0);
    }));

    message!("Gatherer::Main", "Creating thread pool...");
    rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build_global()?;

    #[cfg(target_os = "linux")]
    run_dbus_ipc()?;

    #[cfg(target_os = "macos")]
    run_unix_socket_ipc()?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn run_dbus_ipc() -> Result<(), Box<dyn std::error::Error>> {
    message!("Gatherer::Main", "Setting up D-Bus connection...");
    let c = Arc::new(SyncConnection::new_session()?);

    message!("Gatherer::Main", "Requesting bus name...");
    c.request_name("io.missioncenter.MissionCenter.Gatherer", true, true, true)?;
    message!("Gatherer::Main", "Bus name acquired");

    message!("Gatherer::Main", "Setting up D-Bus proxy...");
    let proxy = c.with_proxy(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        std::time::Duration::from_millis(5000),
    );

    message!("Gatherer::Main", "Setting up D-Bus signal match...");
    let _id = proxy.match_signal(
        |h: OrgFreedesktopDBusNameLost, _: &SyncConnection, _: &dbus::Message| {
            if h.arg0 != "io.missioncenter.MissionCenter.Gatherer" {
                return true;
            }
            message!("Gatherer::Main", "Bus name {} lost, exiting...", &h.arg0);
            std::process::exit(0);
        },
    )?;

    message!("Gatherer::Main", "Setting up D-Bus crossroads...");
    let mut cr = Crossroads::new();
    let iface_token = cr.register("io.missioncenter.MissionCenter.Gatherer", |builder| {
        message!(
            "Gatherer::Main",
            "Registering D-Bus properties and methods..."
        );

        message!(
            "Gatherer::Main",
            "Registering D-Bus property `RefreshInterval`..."
        );
        builder
            .property("RefreshInterval")
            .get_with_cr(|_, _| {
                Ok(SYSTEM_STATE
                    .refresh_interval
                    .load(atomic::Ordering::Relaxed))
            })
            .set_with_cr(|_, _, value| {
                if let Some(value) = value.as_u64() {
                    SYSTEM_STATE
                        .refresh_interval
                        .store(value, atomic::Ordering::Relaxed);
                    Ok(Some(value))
                } else {
                    Err(dbus::MethodErr::failed(&"Invalid value"))
                }
            });

        builder
            .property("CoreCountAffectsPercentages")
            .get_with_cr(|_, _| {
                Ok(SYSTEM_STATE
                    .core_count_affects_percentages
                    .load(atomic::Ordering::Relaxed))
            })
            .set_with_cr(|_, _, value| {
                if let Some(value) = value.as_u64() {
                    let value = value != 0;
                    SYSTEM_STATE
                        .core_count_affects_percentages
                        .store(value, atomic::Ordering::Relaxed);
                    Ok(Some(value))
                } else {
                    Err(dbus::MethodErr::failed(&"Invalid value"))
                }
            });

        message!(
            "Gatherer::Main",
            "Registering D-Bus method `GetCPUStaticInfo`..."
        );
        builder.method_with_cr_custom::<(), (CpuStaticInfo,), &str, _>(
            "GetCPUStaticInfo",
            (),
            ("info",),
            move |mut ctx, _, (): ()| {
                ctx.reply(Ok((SYSTEM_STATE
                    .cpu_info
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .static_info(),)));

                Some(ctx)
            },
        );

        message!(
            "Gatherer::Main",
            "Registering D-Bus method `GetCPUDynamicInfo`..."
        );
        builder.method_with_cr_custom::<(), (CpuDynamicInfo,), &str, _>(
            "GetCPUDynamicInfo",
            (),
            ("info",),
            move |mut ctx, _, (): ()| {
                ctx.reply(Ok((SYSTEM_STATE
                    .cpu_info
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .dynamic_info(),)));

                Some(ctx)
            },
        );

        message!(
            "Gatherer::Main",
            "Registering D-Bus method `GetDisksInfo`..."
        );
        builder.method_with_cr_custom::<(), (Vec<DiskInfo>,), &str, _>(
            "GetDisksInfo",
            (),
            ("info",),
            move |mut ctx, _, (): ()| {
                ctx.reply(Ok((SYSTEM_STATE
                    .disk_info
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .info()
                    .collect::<Vec<_>>(),)));

                Some(ctx)
            },
        );

        message!("Gatherer::Main", "Registering D-Bus method `GetGPUList`...");
        builder.method_with_cr_custom::<(), (Vec<String>,), &str, _>(
            "GetGPUList",
            (),
            ("gpu_list",),
            move |mut ctx, _, (): ()| {
                ctx.reply(Ok((SYSTEM_STATE
                    .gpu_info
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .enumerate()
                    .map(|id| id.to_owned())
                    .collect::<Vec<_>>(),)));

                Some(ctx)
            },
        );

        message!(
            "Gatherer::Main",
            "Registering D-Bus method `GetGPUStaticInfo`..."
        );
        builder.method_with_cr_custom::<(), (Vec<GpuStaticInfo>,), &str, _>(
            "GetGPUStaticInfo",
            (),
            ("info",),
            move |mut ctx, _, (): ()| {
                let gpu_info = SYSTEM_STATE
                    .gpu_info
                    .read()
                    .unwrap_or_else(PoisonError::into_inner);
                ctx.reply(Ok((gpu_info
                    .enumerate()
                    .map(|id| gpu_info.static_info(id).cloned().unwrap())
                    .collect::<Vec<_>>(),)));

                Some(ctx)
            },
        );

        message!(
            "Gatherer::Main",
            "Registering D-Bus method `GetGPUDynamicInfo`..."
        );
        builder.method_with_cr_custom::<(), (Vec<GpuDynamicInfo>,), &str, _>(
            "GetGPUDynamicInfo",
            (),
            ("info",),
            move |mut ctx, _, (): ()| {
                let gpu_info = SYSTEM_STATE
                    .gpu_info
                    .read()
                    .unwrap_or_else(PoisonError::into_inner);
                ctx.reply(Ok((gpu_info
                    .enumerate()
                    .map(|id| gpu_info.dynamic_info(id).cloned().unwrap())
                    .collect::<Vec<_>>(),)));

                Some(ctx)
            },
        );

        message!(
            "Gatherer::Main",
            "Registering D-Bus method `GetFansInfo`..."
        );
        builder.method_with_cr_custom::<(), (Vec<FanInfo>,), &str, _>(
            "GetFansInfo",
            (),
            ("info",),
            move |mut ctx, _, (): ()| {
                ctx.reply(Ok((SYSTEM_STATE
                    .fan_info
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .info()
                    .collect::<Vec<_>>(),)));

                Some(ctx)
            },
        );

        message!(
            "Gatherer::Main",
            "Registering D-Bus method `GetProcesses`..."
        );
        builder.method_with_cr_custom::<(), (Processes,), &str, _>(
            "GetProcesses",
            (),
            ("process_list",),
            move |mut ctx, _, (): ()| {
                ctx.reply(Ok((&*SYSTEM_STATE
                    .processes
                    .write()
                    .unwrap_or_else(PoisonError::into_inner),)));

                Some(ctx)
            },
        );

        message!("Gatherer::Main", "Registering D-Bus method `GetApps`...");
        builder.method_with_cr_custom::<(), (Apps,), &str, _>(
            "GetApps",
            (),
            ("app_list",),
            move |mut ctx, _, (): ()| {
                ctx.reply(Ok((SYSTEM_STATE
                    .apps
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .app_list(),)));

                Some(ctx)
            },
        );

        message!(
            "Gatherer::Main",
            "Registering D-Bus method `GetServices`..."
        );
        builder.method_with_cr_custom::<(), (Vec<Service>,), &str, _>(
            "GetServices",
            (),
            ("service_list",),
            move |mut ctx, _, (): ()| {
                match SYSTEM_STATE
                    .services
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .services()
                {
                    Ok(s) => {
                        ctx.reply(Ok((s,)));
                    }
                    Err(e) => {
                        error!("Gatherer::Main", "Failed to get services: {}", e);
                        ctx.reply::<(Vec<Service>,)>(Ok((vec![],)));
                    }
                }

                Some(ctx)
            },
        );

        message!(
            "Gatherer::Main",
            "Registering D-Bus method `TerminateProcess`..."
        );
        builder.method(
            "TerminateProcess",
            ("process_id",),
            (),
            move |_, _: &mut (), (pid,): (u32,)| {
                execute_no_reply(
                    SYSTEM_STATE.processes.clone(),
                    move |processes| -> Result<(), u8> { Ok(processes.terminate_process(pid)) },
                    "terminating process",
                )
            },
        );

        message!(
            "Gatherer::Main",
            "Registering D-Bus method `KillProcess`..."
        );
        builder.method(
            "KillProcess",
            ("process_id",),
            (),
            move |_, _: &mut (), (pid,): (u32,)| {
                execute_no_reply(
                    SYSTEM_STATE.processes.clone(),
                    move |processes| -> Result<(), u8> { Ok(processes.kill_process(pid)) },
                    "terminating process",
                )
            },
        );

        message!(
            "Gatherer::Main",
            "Registering D-Bus method `EnableService`..."
        );
        builder.method(
            "EnableService",
            ("service_name",),
            (),
            move |_, _: &mut (), (service,): (String,)| {
                execute_no_reply(
                    SYSTEM_STATE.service_controller.clone(),
                    move |sc| {
                        if let Some(sc) = sc.as_ref() {
                            sc.enable_service(&service)
                        } else {
                            Err(ServicesError::MissingServiceController)
                        }
                    },
                    "enabling service",
                )
            },
        );

        message!(
            "Gatherer::Main",
            "Registering D-Bus method `DisableService`..."
        );
        builder.method(
            "DisableService",
            ("service_name",),
            (),
            move |_, _: &mut (), (service,): (String,)| {
                execute_no_reply(
                    SYSTEM_STATE.service_controller.clone(),
                    move |sc| {
                        if let Some(sc) = sc.as_ref() {
                            sc.disable_service(&service)
                        } else {
                            Err(ServicesError::MissingServiceController)
                        }
                    },
                    "disabling service",
                )
            },
        );

        message!(
            "Gatherer::Main",
            "Registering D-Bus method `StartService`..."
        );
        builder.method(
            "StartService",
            ("service_name",),
            (),
            move |_, _: &mut (), (service,): (String,)| {
                execute_no_reply(
                    SYSTEM_STATE.service_controller.clone(),
                    move |sc| {
                        if let Some(sc) = sc.as_ref() {
                            sc.start_service(&service)
                        } else {
                            Err(ServicesError::MissingServiceController)
                        }
                    },
                    "starting service",
                )
            },
        );

        message!(
            "Gatherer::Main",
            "Registering D-Bus method `StopService`..."
        );
        builder.method(
            "StopService",
            ("service_name",),
            (),
            move |_, _: &mut (), (service,): (String,)| {
                execute_no_reply(
                    SYSTEM_STATE.service_controller.clone(),
                    move |sc| {
                        if let Some(sc) = sc.as_ref() {
                            sc.stop_service(&service)
                        } else {
                            Err(ServicesError::MissingServiceController)
                        }
                    },
                    "stopping service",
                )
            },
        );

        message!(
            "Gatherer::Main",
            "Registering D-Bus method `RestartService`..."
        );
        builder.method(
            "RestartService",
            ("service_name",),
            (),
            move |_, _: &mut (), (service,): (String,)| {
                execute_no_reply(
                    SYSTEM_STATE.service_controller.clone(),
                    move |sc| {
                        if let Some(sc) = sc.as_ref() {
                            sc.restart_service(&service)
                        } else {
                            Err(ServicesError::MissingServiceController)
                        }
                    },
                    "restarting service",
                )
            },
        );

        message!(
            "Gatherer::Main",
            "Registering D-Bus method `GetServiceLogs`..."
        );
        builder.method_with_cr_custom::<(String, u32), (String,), &str, _>(
            "GetServiceLogs",
            ("name", "pid"),
            ("service_list",),
            move |mut ctx, _, (name, pid): (String, u32)| {
                match SYSTEM_STATE
                    .services
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .service_logs(&name, std::num::NonZeroU32::new(pid))
                {
                    Ok(s) => {
                        ctx.reply(Ok((s.as_ref().to_owned(),)));
                    }
                    Err(e) => {
                        ctx.reply(Result::<(Vec<Service>,), dbus::MethodErr>::Err(
                            dbus::MethodErr::failed::<String>(&format!(
                                "Failed to get service logs: {e}"
                            )),
                        ));
                    }
                }

                Some(ctx)
            },
        );
    });

    message!(
        "Gatherer::Main",
        "Registering D-Bus interface `org.freedesktop.DBus.Peer`..."
    );
    let peer_itf = cr.register("org.freedesktop.DBus.Peer", |builder| {
        message!(
            "Gatherer::Main",
            "Registering D-Bus method `GetMachineId`..."
        );
        builder.method("GetMachineId", (), ("machine_uuid",), |_, _, (): ()| {
            Ok((std::fs::read_to_string("/var/lib/dbus/machine-id")
                .map_or("UNKNOWN".into(), |s| s.trim().to_owned()),))
        });

        message!("Gatherer::Main", "Registering D-Bus method `Ping`...");
        builder.method("Ping", (), (), |_, _, (): ()| Ok(()));
    });

    message!(
        "Gatherer::Main",
        "Instantiating System and inserting it into Crossroads..."
    );
    cr.insert(DBUS_OBJECT_PATH, &[peer_itf, iface_token], ());

    message!("Gatherer::Main", "Serving D-Bus requests...");

    let cr = Arc::new(Mutex::new(cr));
    c.start_receive(dbus::message::MatchRule::new_method_call(), {
        Box::new(move |msg, conn| {
            cr.lock()
                .unwrap()
                .handle_message(msg, conn)
                .unwrap_or_else(|_| error!("Gatherer::Main", "Failed to handle message"));
            true
        })
    });

    loop {
        c.process(std::time::Duration::from_millis(1000))?;
    }
}

#[cfg(target_os = "linux")]
fn execute_no_reply<SF: Send + Sync + 'static, E: std::fmt::Display>(
    stats: Arc<RwLock<SF>>,
    command: impl FnOnce(&SF) -> Result<(), E> + Send + 'static,
    description: &'static str,
) -> Result<(), dbus::MethodErr> {
    rayon::spawn(move || {
        let stats = match stats.read() {
            Ok(s) => s,
            Err(poisoned_lock) => {
                warning!(
                    "Gatherer::Main",
                    "Lock poisoned while executing command for {}",
                    description
                );
                poisoned_lock.into_inner()
            }
        };

        if let Err(e) = command(&stats) {
            error!("Gatherer::Main", "Failed to execute command: {}", e);
        }
    });

    Ok(())
}

#[cfg(target_os = "macos")]
fn run_unix_socket_ipc() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;

    let socket_path = std::env::var("MC_GATHERER_SOCKET")
        .unwrap_or_else(|_| "/tmp/missioncenter-gatherer.sock".to_string());

    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path)?;

    message!("Gatherer::Main", "Listening on Unix socket: {}", socket_path);

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                std::thread::spawn(move || {
                    let mut len_buf = [0u8; 4];
                    loop {
                        if stream.read_exact(&mut len_buf).is_err() {
                            break;
                        }
                        let msg_len = u32::from_le_bytes(len_buf) as usize;
                        if msg_len == 0 || msg_len > 1024 * 1024 {
                            break;
                        }
                        let mut msg_buf = vec![0u8; msg_len];
                        if stream.read_exact(&mut msg_buf).is_err() {
                            break;
                        }
                        let raw = match std::str::from_utf8(&msg_buf) {
                            Ok(s) => s,
                            Err(_) => break,
                        };
                        // Protocol: "METHOD\0ARG" or "METHOD\0" (no arg)
                        let (method, arg) = match raw.split_once('\0') {
                            Some((m, a)) => (m, a),
                            None => (raw, ""),
                        };
                        let response = handle_ipc_method(method, arg);
                        let resp_len = (response.len() as u32).to_le_bytes();
                        if stream.write_all(&resp_len).is_err() {
                            break;
                        }
                        if stream.write_all(&response).is_err() {
                            break;
                        }
                    }
                });
            }
            Err(e) => {
                error!("Gatherer::Main", "Unix socket accept error: {}", e);
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn handle_ipc_method(cmd: &str, arg: &str) -> Vec<u8> {
    use platform::{
        CpuDynamicInfoExt, CpuInfoExt, CpuStaticInfoExt, DiskInfoExt, DisksInfoExt, FanInfoExt,
        FansInfoExt, GpuDynamicInfoExt, GpuInfoExt, GpuStaticInfoExt, ProcessExt, ProcessesExt,
    };

    match cmd {
        "GetCPUStaticInfo" => {
            let guard = SYSTEM_STATE.cpu_info.read().unwrap_or_else(PoisonError::into_inner);
            let s = guard.static_info();
            serde_json::to_vec(&serde_json::json!({
                "name": s.name(),
                "logical_cpu_count": s.logical_cpu_count(),
                "socket_count": s.socket_count(),
                "base_frequency_khz": s.base_frequency_khz(),
                "is_virtual_machine": s.is_virtual_machine(),
                "l1_combined_cache": s.l1_combined_cache(),
                "l2_cache": s.l2_cache(),
                "l3_cache": s.l3_cache(),
                "l4_cache": s.l4_cache()
            })).unwrap_or_default()
        }
        "GetCPUDynamicInfo" => {
            let guard = SYSTEM_STATE.cpu_info.read().unwrap_or_else(PoisonError::into_inner);
            let d = guard.dynamic_info();
            let per_cpu: Vec<f32> = d.per_logical_cpu_utilization_percent().copied().collect();
            let per_cpu_len = per_cpu.len();
            serde_json::to_vec(&serde_json::json!({
                "overall_utilization_percent": d.overall_utilization_percent(),
                "overall_kernel_utilization_percent": d.overall_kernel_utilization_percent(),
                "per_logical_cpu_utilization_percent": per_cpu,
                "per_logical_cpu_kernel_utilization_percent": vec![0.0f32; per_cpu_len],
                "current_frequency_mhz": d.current_frequency_mhz(),
                "temperature": d.temperature(),
                "process_count": d.process_count(),
                "thread_count": d.thread_count(),
                "uptime_seconds": d.uptime_seconds()
            })).unwrap_or_default()
        }
        "GetDisksInfo" => {
            let guard = SYSTEM_STATE.disk_info.read().unwrap_or_else(PoisonError::into_inner);
            let disks: Vec<_> = guard.info().map(|d| serde_json::json!({
                "id": d.id(), "model": d.model(), "type": d.r#type() as u8,
                "capacity": d.capacity(), "formatted": d.formatted(),
                "system_disk": d.is_system_disk(),
                "busy_percent": d.busy_percent(),
                "response_time_ms": d.response_time_ms(),
                "read_speed": d.read_speed(), "write_speed": d.write_speed()
            })).collect();
            serde_json::to_vec(&disks).unwrap_or_default()
        }
        "GetGPUList" => {
            let guard = SYSTEM_STATE.gpu_info.read().unwrap_or_else(PoisonError::into_inner);
            let ids: Vec<String> = guard.enumerate().map(|s| s.to_owned()).collect();
            serde_json::to_vec(&ids).unwrap_or_default()
        }
        "GetGPUStaticInfo" => {
            let guard = SYSTEM_STATE.gpu_info.read().unwrap_or_else(PoisonError::into_inner);
            let gpus: Vec<_> = guard.enumerate().filter_map(|id| {
                guard.static_info(id).map(|s| serde_json::json!({
                    "id": s.id(), "device_name": s.device_name(),
                    "vendor_id": s.vendor_id(), "device_id": s.device_id(),
                    "total_memory": s.total_memory(),
                    "metal_version": s.metal_version().map(|v| format!("{}.{}", v.major, v.minor))
                }))
            }).collect();
            serde_json::to_vec(&gpus).unwrap_or_default()
        }
        "GetGPUDynamicInfo" => {
            let guard = SYSTEM_STATE.gpu_info.read().unwrap_or_else(PoisonError::into_inner);
            let gpus: Vec<_> = guard.enumerate().filter_map(|id| {
                guard.dynamic_info(id).map(|d| serde_json::json!({
                    "id": d.id(), "temp_celsius": d.temp_celsius(),
                    "util_percent": d.util_percent(), "clock_speed_mhz": d.clock_speed_mhz(),
                    "used_memory": d.used_memory(), "free_memory": d.free_memory(),
                    "encoder_percent": d.encoder_percent(), "decoder_percent": d.decoder_percent()
                }))
            }).collect();
            serde_json::to_vec(&gpus).unwrap_or_default()
        }
        "GetFansInfo" => {
            let guard = SYSTEM_STATE.fan_info.read().unwrap_or_else(PoisonError::into_inner);
            let fans: Vec<_> = guard.info().map(|f| serde_json::json!({
                "fan_label": f.fan_label(), "rpm": f.rpm(),
                "percent_vroomimg": f.percent_vroomimg(), "max_speed": f.max_speed()
            })).collect();
            serde_json::to_vec(&fans).unwrap_or_default()
        }
        "GetProcesses" => {
            let guard = SYSTEM_STATE.processes.read().unwrap_or_else(PoisonError::into_inner);
            let list: Vec<_> = guard.process_list().values().map(|p| {
                use crate::platform::ProcessExt;
                let cmd: Vec<&str> = p.cmd().collect();
                serde_json::json!({
                    "name": p.name(), "pid": p.pid(), "parent": p.parent(), "exe": p.exe(),
                    "cmd": if cmd.is_empty() { vec![p.exe()] } else { cmd },
                    "task_count": p.task_count(),
                    "usage_stats": {
                        "cpu_usage": p.usage_stats().cpu_usage,
                        "memory_usage": p.usage_stats().memory_usage,
                        "disk_usage": p.usage_stats().disk_usage,
                        "network_usage": 0.0f32,
                        "gpu_usage": p.usage_stats().gpu_usage,
                        "gpu_memory_usage": p.usage_stats().gpu_memory_usage
                    }
                })
            }).collect();
            serde_json::to_vec(&list).unwrap_or_default()
        }
        "GetApps" => {
            use platform::AppExt;
            let guard = SYSTEM_STATE.apps.read().unwrap_or_else(PoisonError::into_inner);
            let list: Vec<_> = guard.app_list().iter().map(|a| serde_json::json!({
                "name": a.name(), "icon": a.icon(), "id": a.id(),
                "command": a.command(),
                "pids": a.pids().collect::<Vec<_>>()
            })).collect();
            serde_json::to_vec(&list).unwrap_or_default()
        }
        "GetServices" => {
            use platform::ServicesExt;
            let guard = SYSTEM_STATE.services.read().unwrap_or_else(PoisonError::into_inner);
            let list: Vec<_> = guard.services().unwrap_or_default().iter().map(|s| {
                use platform::ServiceExt;
                serde_json::json!({
                    "name": s.name(), "description": s.description(),
                    "enabled": s.enabled(), "running": s.running(), "failed": s.failed(),
                    "pid": s.pid().map(|p| p.get()),
                    "user": s.user(), "group": s.group()
                })
            }).collect();
            serde_json::to_vec(&list).unwrap_or_default()
        }
        "TerminateProcess" => {
            if let Ok(pid) = arg.parse::<u32>() {
                SYSTEM_STATE.processes.read().unwrap_or_else(PoisonError::into_inner).terminate_process(pid);
            }
            b"{}".to_vec()
        }
        "KillProcess" => {
            if let Ok(pid) = arg.parse::<u32>() {
                SYSTEM_STATE.processes.read().unwrap_or_else(PoisonError::into_inner).kill_process(pid);
            }
            b"{}".to_vec()
        }
        "SetRefreshInterval" => {
            if let Ok(ms) = arg.parse::<u64>() {
                SYSTEM_STATE.refresh_interval.store(ms, atomic::Ordering::Relaxed);
            }
            b"{}".to_vec()
        }
        "SetCoreCountAffectsPercentages" => {
            let val = arg == "true" || arg == "1";
            SYSTEM_STATE.core_count_affects_percentages.store(val, atomic::Ordering::Relaxed);
            b"{}".to_vec()
        }
        _ => {
            warning!("Gatherer::Main", "Unknown IPC method: {}", cmd);
            b"{}".to_vec()
        }
    }
}
