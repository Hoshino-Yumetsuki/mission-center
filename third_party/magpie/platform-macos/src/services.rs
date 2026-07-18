/* src/services.rs
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

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::num::NonZeroU32;
use std::process::Command;

use magpie_platform::services::Service;

pub struct ServiceCache {
    user: HashMap<u64, Service>,
    system: HashMap<u64, Service>,
}

impl magpie_platform::services::ServiceCache for ServiceCache {
    fn new() -> Self {
        Self {
            user: HashMap::new(),
            system: HashMap::new(),
        }
    }

    fn refresh(&mut self) {
        // launchctl can return hundreds of jobs; dumping them into the Services
        // page currently trips a GTK list-model crash on macOS
        // (gtk_list_item_manager_ensure_items via set_incremental). Cap hard.
        // ponytail: empty system list + capped user list; full dump once UI fixed.
        let mut user = parse_launchctl_list();
        if user.len() > 64 {
            user = user.into_iter().take(64).collect();
        }
        self.user = user;
        self.system = HashMap::new();
    }

    fn user_entries(&self) -> &HashMap<u64, Service> {
        &self.user
    }

    fn system_entries(&self) -> &HashMap<u64, Service> {
        &self.system
    }
}

pub struct ServiceManager;

impl magpie_platform::services::ServiceManager for ServiceManager {
    type ServiceCache = ServiceCache;

    fn new() -> Self {
        Self
    }

    fn logs(
        &self,
        _sc: &Self::ServiceCache,
        _id: u64,
        _pid: Option<NonZeroU32>,
    ) -> Option<String> {
        // ponytail: log stream is heavy; wire log show --predicate later if UI needs it.
        None
    }

    fn start(&self, sc: &Self::ServiceCache, id: u64) {
        if let Some(name) = service_name(sc, id) {
            // Best-effort: kickstart in current user GUI domain.
            let _ = Command::new("/bin/launchctl")
                .args(["kickstart", "-k", &format!("gui/{}/{}", uid(), name)])
                .output();
        }
    }

    fn stop(&self, sc: &Self::ServiceCache, id: u64) {
        if let Some(name) = service_name(sc, id) {
            let _ = Command::new("/bin/launchctl")
                .args(["kill", "SIGTERM", &format!("gui/{}/{}", uid(), name)])
                .output();
        }
    }

    fn restart(&self, sc: &Self::ServiceCache, id: u64) {
        self.stop(sc, id);
        self.start(sc, id);
    }

    fn enable(&self, sc: &Self::ServiceCache, id: u64) {
        if let Some(name) = service_name(sc, id) {
            let _ = Command::new("/bin/launchctl")
                .args(["enable", &format!("gui/{}/{}", uid(), name)])
                .output();
        }
    }

    fn disable(&self, sc: &Self::ServiceCache, id: u64) {
        if let Some(name) = service_name(sc, id) {
            let _ = Command::new("/bin/launchctl")
                .args(["disable", &format!("gui/{}/{}", uid(), name)])
                .output();
        }
    }
}

fn service_name(sc: &ServiceCache, id: u64) -> Option<String> {
    sc.user
        .get(&id)
        .or_else(|| sc.system.get(&id))
        .map(|s| s.name.clone())
}

fn uid() -> u32 {
    unsafe { libc::getuid() }
}

fn service_id(name: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    hasher.finish()
}

/// User-domain jobs from `launchctl list`.
fn parse_launchctl_list() -> HashMap<u64, Service> {
    let out = match Command::new("/bin/launchctl").arg("list").output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => return HashMap::new(),
    };

    let mut map = HashMap::new();
    for line in out.lines().skip(1) {
        // PID\tStatus\tLabel
        let mut parts = line.splitn(3, '\t');
        let pid_s = parts.next().unwrap_or("-");
        let status_s = parts.next().unwrap_or("0");
        let label = parts.next().unwrap_or("").trim();
        if label.is_empty() {
            continue;
        }

        let pid = if pid_s == "-" {
            None
        } else {
            pid_s.parse::<u32>().ok().filter(|&p| p > 0)
        };
        let status: i32 = status_s.parse().unwrap_or(0);
        let running = pid.is_some();
        let failed = status != 0 && !running;

        let id = service_id(label);
        map.insert(
            id,
            Service {
                id,
                name: label.to_string(),
                description: None,
                enabled: true, // listed jobs are loaded
                running,
                failed,
                pid,
                user: None,
                group: None,
                file_path: None,
            },
        );
    }
    map
}

/// System domain labels from `launchctl print system` (no root required for listing).
#[allow(dead_code)] // reserved for when Services UI can handle full dumps
fn parse_system_services() -> HashMap<u64, Service> {
    let out = match Command::new("/bin/launchctl")
        .args(["print", "system"])
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => return HashMap::new(),
    };

    let mut map = HashMap::new();
    // Lines like: `"com.apple.ftpd" => disabled`
    for line in out.lines() {
        let line = line.trim();
        if !line.starts_with('"') {
            continue;
        }
        let Some(end_q) = line[1..].find('"') else {
            continue;
        };
        let name = &line[1..1 + end_q];
        if name.is_empty() {
            continue;
        }
        let enabled = if line.contains("=> enabled") {
            true
        } else if line.contains("=> disabled") {
            false
        } else {
            continue;
        };

        let id = service_id(name);
        map.entry(id).or_insert_with(|| Service {
            id,
            name: name.to_string(),
            description: None,
            enabled,
            running: false, // print system doesn't expose pid cheaply
            failed: false,
            pid: None,
            user: None,
            group: None,
            file_path: None,
        });
    }
    map
}
