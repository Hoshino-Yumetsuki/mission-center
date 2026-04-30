#![allow(dead_code)]
/* sys_info_v2/gatherer/src/platform/macos/apps.rs
 *
 * Copyright 2024 Mission Center Contributors
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use crate::dbus_shim::{Append, Arg, ArgType, IterAppend, Signature};
use std::sync::Arc;

use crate::platform::apps::{AppExt, AppsExt};
use crate::platform::ProcessExt;

#[derive(Debug, Clone)]
pub struct MacosApp {
    name: Arc<str>,
    icon: Option<Arc<str>>,
    id: Arc<str>,
    command: Arc<str>,
    pids: Vec<u32>,
}

impl Default for MacosApp {
    fn default() -> Self {
        Self {
            name: Arc::from(""),
            icon: None,
            id: Arc::from(""),
            command: Arc::from(""),
            pids: vec![],
        }
    }
}

impl<'a> AppExt<'a> for MacosApp {
    type Iter = std::slice::Iter<'a, u32>;

    fn name(&self) -> &str { self.name.as_ref() }
    fn icon(&self) -> Option<&str> { self.icon.as_deref() }
    fn id(&self) -> &str { self.id.as_ref() }
    fn command(&self) -> &str { self.command.as_ref() }
    fn pids(&'a self) -> Self::Iter { self.pids.iter() }
}

#[derive(Default)]
pub struct MacosApps {
    apps: Vec<MacosApp>,
}

impl MacosApps {
    pub fn new() -> Self { Self::default() }
}

fn bundle_path_from_exe(exe: &str) -> Option<String> {
    let lower = exe.to_lowercase();
    let pos = lower.find(".app/")?;
    Some(exe[..pos + 4].to_string())
}

fn icns_to_png(icns_path: &str) -> Option<String> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    icns_path.hash(&mut hasher);
    let hash = hasher.finish();

    let cache_dir = std::env::temp_dir().join("missioncenter-icons");
    let _ = std::fs::create_dir_all(&cache_dir);
    let png_path = cache_dir.join(format!("{:016x}.png", hash));

    if png_path.exists() {
        return Some(png_path.to_string_lossy().into_owned());
    }

    let status = std::process::Command::new("/usr/bin/sips")
        .args(["-s", "format", "png", icns_path, "--out", png_path.to_str()?])
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
    if !std::path::Path::new(&plist_path).exists() {
        return None;
    }

    let out = std::process::Command::new("/usr/bin/plutil")
        .args(["-convert", "json", "-o", "-", &plist_path])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }

    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;

    let bundle_id = json.get("CFBundleIdentifier")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())?;

    let name = json.get("CFBundleName")
        .or_else(|| json.get("CFBundleDisplayName"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            std::path::Path::new(bundle_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string()
        });

    let icon = json.get("CFBundleIconFile")
        .or_else(|| json.get("CFBundleIconName"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .and_then(|icon_file| {
            let icon_file = if icon_file.ends_with(".icns") {
                icon_file.to_string()
            } else {
                format!("{}.icns", icon_file)
            };
            let icns_path = format!("{}/Contents/Resources/{}", bundle_path, icon_file);
            if std::path::Path::new(&icns_path).exists() {
                icns_to_png(&icns_path)
            } else {
                None
            }
        });

    Some((bundle_id, name, icon))
}

impl<'a> AppsExt<'a> for MacosApps {
    type A = MacosApp;
    type P = super::processes::MacosProcess;

    fn refresh_cache(&mut self, processes: &std::collections::HashMap<u32, Self::P>) {
        self.apps.clear();

        let mut bundle_map: std::collections::HashMap<String, (String, Option<String>, String, Vec<u32>)> =
            std::collections::HashMap::new();

        for (pid, proc) in processes {
            let exe = proc.exe();
            if exe.is_empty() {
                continue;
            }

            let bundle_path = match bundle_path_from_exe(exe) {
                Some(p) => p,
                None => continue,
            };

            let entry = bundle_map.entry(bundle_path.clone()).or_insert_with(|| {
                match read_bundle_info(&bundle_path) {
                    Some((id, _name, icon)) => (id, icon, bundle_path.clone(), vec![]),
                    None => (String::new(), None, bundle_path.clone(), vec![]),
                }
            });

            entry.3.push(*pid);
        }

        for (bundle_path, (id, icon, command, pids)) in bundle_map {
            if id.is_empty() || pids.is_empty() {
                continue;
            }

            let name = std::path::Path::new(&bundle_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(id.as_str())
                .to_string();

            self.apps.push(MacosApp {
                name: Arc::from(name.as_str()),
                icon: icon.map(|i| Arc::from(i.as_str())),
                id: Arc::from(id.as_str()),
                command: Arc::from(command.as_str()),
                pids,
            });
        }

        self.apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    }

    fn app_list(&self) -> &[Self::A] {
        &self.apps
    }
}

impl Arg for MacosApp {
    const ARG_TYPE: ArgType = ArgType::Struct;
    fn signature() -> Signature { Signature::from("") }
}
impl Append for MacosApp {
    fn append_by_ref(&self, _: &mut IterAppend) {}
}

impl Arg for MacosApps {
    const ARG_TYPE: ArgType = ArgType::Struct;
    fn signature() -> Signature { Signature::from("") }
}
impl Append for MacosApps {
    fn append_by_ref(&self, _: &mut IterAppend) {}
}
