/* src/about.rs
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

use std::process::Command;

use magpie_platform::about::{About, AboutDeInfo, AboutDeviceInfo, AboutOsInfo};

use crate::util::sysctl_string;

pub struct AboutCache {
    about: About,
    loaded: bool,
}

impl magpie_platform::about::AboutCache for AboutCache {
    fn new() -> Self {
        Self {
            about: About::default(),
            loaded: false,
        }
    }

    fn refresh(&mut self) {
        // Static-ish: refresh once is enough for about.
        if self.loaded {
            return;
        }
        self.about = collect_about();
        self.loaded = true;
    }

    fn about(&self) -> &About {
        &self.about
    }
}

fn collect_about() -> About {
    let mut about = About::default();
    about.os_info = collect_os_info();
    about.de_info = collect_de_info();
    about.device_info = collect_device_info();
    about
}

fn collect_os_info() -> AboutOsInfo {
    let mut info = AboutOsInfo::default();

    let product_name = sw_vers("ProductName").unwrap_or_else(|| "macOS".into());
    let product_version = sw_vers("ProductVersion");
    let build_version = sw_vers("BuildVersion");

    info.name = Some(product_name.clone());
    info.pretty_name = match (&product_version, &build_version) {
        (Some(v), Some(b)) => Some(format!("{product_name} {v} ({b})")),
        (Some(v), None) => Some(format!("{product_name} {v}")),
        _ => Some(product_name.clone()),
    };
    info.id = Some("macos".into());
    info.id_like = Some("darwin".into());
    info.version_id = product_version.clone();
    info.version = product_version.or(build_version.clone());
    info.os_type = Some("Darwin".into());
    info.os_architecture = sysctl_string("hw.machine").or_else(|| uname_field("-m"));
    info.kernel_release = sysctl_string("kern.osrelease").or_else(|| uname_field("-r"));
    info.kernel_version = sysctl_string("kern.version").or_else(|| uname_field("-v"));
    info.home_url = Some("https://www.apple.com/macos/".into());
    info.package_manager = detect_package_manager();

    info
}

fn collect_de_info() -> AboutDeInfo {
    AboutDeInfo {
        desktop_environment: Some("Aqua".into()),
        version: sw_vers("ProductVersion"),
        windowing_system: Some("Quartz".into()),
        session_id: None,
        session_type: Some("gui".into()),
        virtual_terminal: None,
    }
}

fn collect_device_info() -> AboutDeviceInfo {
    let hostname = sysctl_string("kern.hostname")
        .or_else(|| command_stdout(&["/bin/hostname"]))
        .map(|s| s.trim().to_string());

    let model = hardware_field("Model Name")
        .or_else(|| sysctl_string("hw.model"));

    AboutDeviceInfo {
        hostname,
        vendor: Some("Apple".into()),
        model,
    }
}

fn sw_vers(key: &str) -> Option<String> {
    let out = Command::new("/usr/bin/sw_vers")
        .arg(format!("-{key}"))
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

fn uname_field(flag: &str) -> Option<String> {
    command_stdout(&["/usr/bin/uname", flag]).map(|s| s.trim().to_string())
}

fn hardware_field(label: &str) -> Option<String> {
    let out = Command::new("/usr/sbin/system_profiler")
        .args(["SPHardwareDataType"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let prefix = format!("{label}:");
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(&prefix) {
            let v = rest.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn detect_package_manager() -> Option<String> {
    for (bin, name) in [
        ("/opt/homebrew/bin/brew", "Homebrew"),
        ("/usr/local/bin/brew", "Homebrew"),
        ("/opt/local/bin/port", "MacPorts"),
    ] {
        if std::path::Path::new(bin).exists() {
            return Some(name.into());
        }
    }
    None
}

fn command_stdout(cmd: &[&str]) -> Option<String> {
    let (bin, args) = cmd.split_first()?;
    let out = Command::new(bin).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}
