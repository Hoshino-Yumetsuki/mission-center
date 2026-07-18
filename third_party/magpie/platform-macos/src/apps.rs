/* src/apps.rs
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
use std::path::Path;
use std::process::Command;

use magpie_platform::apps::{App, Icon, IconKind};
use magpie_platform::processes::Process;
use magpie_platform::Mutex;

pub struct AppCache {
    apps: Vec<App>,
    icons: HashMap<String, Icon>,
    unresolved_icons: HashMap<String, String>,
}

impl magpie_platform::apps::AppCache for AppCache {
    fn new() -> Self {
        Self {
            apps: Vec::new(),
            icons: HashMap::new(),
            unresolved_icons: HashMap::new(),
        }
    }

    fn refresh(&mut self, processes: &Mutex<HashMap<u32, Process>>) {
        self.apps.clear();

        // bundle_path -> (id, name, icon_png_path, command, pids)
        let mut bundle_map: HashMap<String, (String, String, Option<String>, String, Vec<u32>)> =
            HashMap::new();

        {
            let processes = processes.lock();
            for (pid, proc) in processes.iter() {
                if proc.exe.is_empty() {
                    continue;
                }
                let Some(bundle_path) = bundle_path_from_exe(&proc.exe) else {
                    continue;
                };

                let entry = bundle_map.entry(bundle_path.clone()).or_insert_with(|| {
                    match read_bundle_info(&bundle_path) {
                        Some((id, name, icon)) => {
                            (id, name, icon, bundle_path.clone(), Vec::new())
                        }
                        None => (
                            String::new(),
                            String::new(),
                            None,
                            bundle_path.clone(),
                            Vec::new(),
                        ),
                    }
                });
                entry.4.push(*pid);
            }
        }

        for (_bundle_path, (id, name, icon, command, pids)) in bundle_map {
            if id.is_empty() || pids.is_empty() {
                continue;
            }

            if let Some(icon_path) = icon {
                if !self.icons.contains_key(&id) && !self.unresolved_icons.contains_key(&id) {
                    self.unresolved_icons.insert(id.clone(), icon_path);
                }
            } else if !self.icons.contains_key(&id) {
                self.icons.insert(
                    id.clone(),
                    Icon {
                        icon: Some(IconKind::Empty(Default::default())),
                    },
                );
            }

            self.apps.push(App {
                id,
                name,
                command,
                pids,
            });
        }

        self.apps
            .sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        if !self.unresolved_icons.is_empty() {
            self.refresh_icons();
        }
    }

    fn refresh_icons(&mut self) {
        for (app_id, path) in self.unresolved_icons.drain() {
            let icon = match std::fs::read(&path) {
                Ok(bytes) => Icon {
                    icon: Some(IconKind::Data(bytes)),
                },
                Err(e) => {
                    log::debug!("Failed to read icon {path} for {app_id}: {e}");
                    Icon {
                        icon: Some(IconKind::Id(path)),
                    }
                }
            };
            self.icons.insert(app_id, icon);
        }
    }

    fn cached_entries(&self) -> &[App] {
        &self.apps
    }

    fn cached_icons(&self) -> HashMap<String, Icon> {
        self.icons.clone()
    }
}

fn bundle_path_from_exe(exe: &str) -> Option<String> {
    let lower = exe.to_lowercase();
    let pos = lower.find(".app/")?;
    Some(exe[..pos + 4].to_string())
}

fn plutil_raw(plist_path: &str, key: &str) -> Option<String> {
    let out = Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-", plist_path])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn icns_to_png(icns_path: &str) -> Option<String> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    icns_path.hash(&mut hasher);
    let hash = hasher.finish();

    let cache_dir = std::env::temp_dir().join("missioncenter-icons");
    let _ = std::fs::create_dir_all(&cache_dir);
    let png_path = cache_dir.join(format!("{:016x}.png", hash));

    if png_path.exists() {
        return Some(png_path.to_string_lossy().into_owned());
    }

    let status = Command::new("/usr/bin/sips")
        .args([
            "-s",
            "format",
            "png",
            icns_path,
            "--out",
            png_path.to_str()?,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;

    if status.success() && png_path.exists() {
        Some(png_path.to_string_lossy().into_owned())
    } else {
        None
    }
}

fn read_bundle_info(bundle_path: &str) -> Option<(String, String, Option<String>)> {
    let plist_path = format!("{}/Contents/Info.plist", bundle_path);
    if !Path::new(&plist_path).exists() {
        return None;
    }

    let bundle_id = plutil_raw(&plist_path, "CFBundleIdentifier")?;

    let name = plutil_raw(&plist_path, "CFBundleName")
        .or_else(|| plutil_raw(&plist_path, "CFBundleDisplayName"))
        .unwrap_or_else(|| {
            Path::new(bundle_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&bundle_id)
                .to_string()
        });

    let icon = plutil_raw(&plist_path, "CFBundleIconFile")
        .or_else(|| plutil_raw(&plist_path, "CFBundleIconName"))
        .and_then(|icon_file| {
            let icon_file = if icon_file.ends_with(".icns") {
                icon_file
            } else {
                format!("{}.icns", icon_file)
            };
            let icns_path = format!("{}/Contents/Resources/{}", bundle_path, icon_file);
            if Path::new(&icns_path).exists() {
                icns_to_png(&icns_path)
            } else {
                None
            }
        });

    Some((bundle_id, name, icon))
}
