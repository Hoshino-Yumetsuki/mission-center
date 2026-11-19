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
    let Ok(output) = Command::new("/usr/sbin/ioreg")
        .args(["-a", "-r", "-c", "AppleSmartBattery", "-d", "1"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let Ok(plist::Value::Array(entries)) = plist::Value::from_reader_xml(&output.stdout[..]) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|v| parse_battery(v.as_dictionary()?))
        .collect()
}

fn parse_battery(entry: &plist::Dictionary) -> Option<Battery> {
    use plist::Value;
    let data = entry.get("BatteryData").and_then(Value::as_dictionary);
    let field = |key: &str| entry.get(key).or_else(|| data?.get(key));
    let number = |key: &str| field(key)?.as_unsigned_integer();
    let flag = |key: &str| field(key)?.as_boolean();
    let string = |key: &str| Some(field(key)?.as_string()?.to_owned());
    if flag("BatteryInstalled") == Some(false) {
        return None;
    }

    let raw_current = number("AppleRawCurrentCapacity");
    let raw_max = number("AppleRawMaxCapacity");
    let ratio = |value: Option<u64>, max: Option<u64>| {
        let max = max.filter(|&m| m > 0)?;
        Some((value? as f32 / max as f32).min(1.0))
    };
    // The public pair can be percent/100 or charge/full-charge. Never mix the pairs.
    let percentage = ratio(number("CurrentCapacity"), number("MaxCapacity"))
        .or_else(|| ratio(raw_current, raw_max))?;
    let state = match (
        flag("IsCharging"),
        flag("FullyCharged"),
        flag("ExternalConnected"),
    ) {
        (Some(true), _, _) => Some(BatteryState::Charging),
        (_, Some(true), _) => Some(BatteryState::FullyCharged),
        (Some(false), _, Some(true)) => Some(BatteryState::PendingCharge),
        (_, _, Some(false)) if percentage == 0.0 => Some(BatteryState::Empty),
        (_, _, Some(false)) => Some(BatteryState::Discharging),
        _ => None,
    };
    let minutes = |key: &str| {
        number(key)
            .filter(|&m| m > 0 && m < 65535)
            .map(|m| (60 * m) as u32)
    };
    let voltage_mv = number("Voltage")
        .or_else(|| number("AppleRawBatteryVoltage"))
        .filter(|&v| v > 0);
    let voltage = voltage_mv.map(|v| v as f32 / 1000.0);
    let current = field("InstantAmperage").or_else(|| field("Amperage"));
    // ioreg can encode a negative mA reading as an unsigned two's complement word.
    let current = current.and_then(|v| {
        v.as_signed_integer()
            .and_then(|a| i32::try_from(a).ok())
            .or_else(|| Some(u32::try_from(v.as_unsigned_integer()?).ok()? as i32))
            .or_else(|| i32::try_from(v.as_unsigned_integer()? as i64).ok())
    });
    // Rated Wh require design voltage, not the pack's changing terminal voltage.
    let design_mv = number("DesignVoltage").filter(|&v| v > 0);
    let design_cap = number("DesignCapacity");
    Some(Battery {
        name: entry.get("IORegistryEntryName")?.as_string()?.into(),
        vendor: string("Manufacturer"),
        model: string("DeviceName"),
        serial: string("Serial"),
        kind: Some(BatteryType::Bat as i32),
        power_supply: Some(true),
        energy_full: charge_to_energy(raw_max, design_mv),
        energy_full_design: charge_to_energy(design_cap, design_mv),
        capacity: ratio(number("NominalChargeCapacity").or(raw_max), design_cap),
        charge_cycles: number("CycleCount").and_then(|n| n.try_into().ok()),
        percentage,
        energy: charge_to_energy(raw_current, voltage_mv),
        voltage,
        power: current
            .zip(voltage)
            .map(|(a, v)| (a as f32).abs() * v / 1000.0),
        time_to_full: (state == Some(BatteryState::Charging))
            .then(|| minutes("AvgTimeToFull").or_else(|| minutes("TimeRemaining")))
            .flatten(),
        time_to_empty: (state == Some(BatteryState::Discharging))
            .then(|| minutes("AvgTimeToEmpty").or_else(|| minutes("TimeRemaining")))
            .flatten(),
        state: state.map(|s| s as i32),
        temp: number("Temperature").map(|t| t as f32 / 100.0),
        ..Default::default()
    })
}

fn charge_to_energy(charge_mah: Option<u64>, voltage_mv: Option<u64>) -> Option<u32> {
    u32::try_from(charge_mah?.checked_mul(voltage_mv?)? / 1000).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn charge_is_a_ratio_and_missing_charge_is_not_full() {
        let mut entry = plist::Dictionary::new();
        entry.insert("IORegistryEntryName".into(), "AppleSmartBattery".into());
        entry.insert("MaxCapacity".into(), 100.into());
        assert!(parse_battery(&entry).is_none());
        entry.insert("CurrentCapacity".into(), 100.into());
        entry.insert("IsCharging".into(), true.into());
        assert_eq!(parse_battery(&entry).unwrap().percentage * 100.0, 100.0);
        entry.insert("CurrentCapacity".into(), 3000.into());
        entry.insert("MaxCapacity".into(), 6000.into());
        entry.insert("Voltage".into(), 12000.into());
        entry.insert("InstantAmperage".into(), (u64::MAX - 999).into());
        let b = parse_battery(&entry).unwrap();
        assert_eq!((b.percentage, b.power), (0.5, Some(12.0)));
        assert_eq!(b.energy_full_design, None);
    }
}
