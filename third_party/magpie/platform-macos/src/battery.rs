/* src/battery.rs
 *
 * Copyright 2026 Mission Center Developers
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

use magpie_platform::battery::{Battery, BatteryState, BatteryType};

pub struct BatteryCache {
    batteries: Vec<Battery>,
}

impl magpie_platform::battery::BatteryCache for BatteryCache {
    fn new() -> Self {
        Self {
            batteries: Vec::new(),
        }
    }

    fn refresh(&mut self) {
        self.batteries = read_smart_batteries();
    }

    fn cached_entries(&self) -> &[Battery] {
        &self.batteries
    }
}

fn read_smart_batteries() -> Vec<Battery> {
    let text = Command::new("/usr/sbin/ioreg")
        .args(["-r", "-c", "AppleSmartBattery", "-d", "1"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();

    if text.trim().is_empty() || !text.contains("AppleSmartBattery") {
        return Vec::new();
    }

    // Desktop Macs without battery return empty registry class.
    let installed = ioreg_bool(&text, "BatteryInstalled").unwrap_or(true);
    if !installed {
        return Vec::new();
    }

    let percentage = ioreg_u64(&text, "CurrentCapacity")
        .or_else(|| ioreg_u64(&text, "MaxCapacity"))
        .unwrap_or(0) as f32;

    let design_cap = ioreg_u64(&text, "DesignCapacity").unwrap_or(0);
    let nom_cap = ioreg_u64(&text, "NominalChargeCapacity").unwrap_or(0);
    let raw_current = ioreg_u64(&text, "AppleRawCurrentCapacity").unwrap_or(0);
    let cycles = ioreg_u64(&text, "CycleCount").map(|c| c as u32);
    let voltage_mv = ioreg_u64(&text, "Voltage").or_else(|| ioreg_u64(&text, "AppleRawBatteryVoltage"));
    let voltage = voltage_mv.map(|mv| mv as f32 / 1000.0);

    // Temperature is 0.01 °C on Apple SMC/IOKit path.
    let temp = ioreg_u64(&text, "Temperature").map(|t| t as f32 / 100.0);

    let is_charging = ioreg_bool(&text, "IsCharging").unwrap_or(false);
    let fully_charged = ioreg_bool(&text, "FullyCharged").unwrap_or(false);
    let external = ioreg_bool(&text, "ExternalConnected").unwrap_or(false);

    let state = if fully_charged || (percentage >= 100.0 && external) {
        Some(BatteryState::FullyCharged as i32)
    } else if is_charging {
        Some(BatteryState::Charging as i32)
    } else if percentage <= 0.0 {
        Some(BatteryState::Empty as i32)
    } else {
        Some(BatteryState::Discharging as i32)
    };

    // TimeRemaining / AvgTimeToEmpty are minutes; 65535 means unknown.
    let time_to_empty = ioreg_u64(&text, "AvgTimeToEmpty")
        .or_else(|| ioreg_u64(&text, "TimeRemaining"))
        .filter(|&m| m > 0 && m < 65535)
        .map(|m| (m * 60) as u32);
    let time_to_full = ioreg_u64(&text, "AvgTimeToFull")
        .filter(|&m| m > 0 && m < 65535)
        .map(|m| (m * 60) as u32);

    // InstantAmperage is signed; IOKit dumps it as unsigned 64-bit.
    let power = ioreg_i64(&text, "InstantAmperage")
        .or_else(|| ioreg_i64(&text, "Amperage"))
        .and_then(|ma| {
            let v = voltage?;
            // mA * V / 1000 = W
            Some((ma as f32 / 1000.0) * v)
        });

    let serial = ioreg_string(&text, "Serial");
    let model = ioreg_string(&text, "DeviceName");

    let capacity = if design_cap > 0 && nom_cap > 0 {
        Some((nom_cap as f32 / design_cap as f32) * 100.0)
    } else {
        None
    };

    vec![Battery {
        name: "InternalBattery-0".into(),
        vendor: Some("Apple".into()),
        model,
        serial,
        kind: Some(BatteryType::Bat as i32),
        technology: None,
        power_supply: Some(true),
        energy_empty: Some(0),
        // mAh-ish values; Linux path uses mWh. Best-effort raw numbers.
        energy_full: (nom_cap > 0).then_some(nom_cap as u32),
        energy_full_design: (design_cap > 0).then_some(design_cap as u32),
        capacity,
        voltage_min_design: None,
        voltage_max_design: None,
        charge_cycles: cycles,
        percentage,
        energy: (raw_current > 0).then_some(raw_current as u32),
        voltage,
        power,
        time_to_full,
        time_to_empty,
        icon_name: None,
        state,
        temp,
        charge_threshold_enabled: false,
        charge_threshold_supported: 0,
        charge_start_threshold: None,
        charge_end_threshold: None,
        history: Vec::new(),
        history_changed: false,
    }]
}

fn ioreg_u64(text: &str, key: &str) -> Option<u64> {
    // Match top-level `"Key" = value` (not nested inside other dicts as well as possible).
    let search = format!("\"{key}\" = ");
    let pos = text.find(&search)?;
    let rest = text[pos + search.len()..].trim_start();
    // Skip non-numeric (arrays/dicts)
    if !rest.starts_with(|c: char| c.is_ascii_digit() || c == '-') {
        return None;
    }
    let num: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    num.parse().ok()
}

fn ioreg_i64(text: &str, key: &str) -> Option<i64> {
    let search = format!("\"{key}\" = ");
    let pos = text.find(&search)?;
    let rest = text[pos + search.len()..].trim_start();
    let num: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    let raw: u64 = num.parse().ok()?;
    // IOKit often prints signed 16/32-bit values as large unsigned.
    if raw > i64::MAX as u64 {
        // interpret as i64 bit pattern truncated from u64
        Some(raw as i64)
    } else if raw > 0x7FFF_FFFF {
        Some(raw as i32 as i64)
    } else {
        Some(raw as i64)
    }
}

fn ioreg_bool(text: &str, key: &str) -> Option<bool> {
    let search = format!("\"{key}\" = ");
    let pos = text.find(&search)?;
    let rest = text[pos + search.len()..].trim_start();
    if rest.starts_with("Yes") {
        Some(true)
    } else if rest.starts_with("No") {
        Some(false)
    } else {
        None
    }
}

fn ioreg_string(text: &str, key: &str) -> Option<String> {
    let search = format!("\"{key}\" = \"");
    let pos = text.find(&search)?;
    let rest = &text[pos + search.len()..];
    let end = rest.find('"')?;
    let s = &rest[..end];
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}
