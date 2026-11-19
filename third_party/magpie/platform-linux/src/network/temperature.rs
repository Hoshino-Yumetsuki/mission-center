/* src/network/temperature.rs
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

use std::path::Path;

use glob::glob;

// Reported by iwlwifi while its firmware is not running
const THERMAL_TEMP_INVALID: i64 = -274_000;

pub fn temperature_milli_k(if_name: &str) -> Option<u32> {
    let base = format!("/sys/class/net/{if_name}");

    // mt76, ath11k and ath12k register the sensor on the wiphy (ath12k once per radio),
    // ath10k and some wired adapters on the parent device
    max_milli_c(&format!("{base}/phy80211/hwmon[0-9]*/temp1_input"))
        .or_else(|| max_milli_c(&format!("{base}/device/hwmon/hwmon[0-9]*/temp1_input")))
        .or_else(|| iwlwifi_milli_c(&base))
        .map(|milli_c| (milli_c + crate::util::MK_TO_0_C) as u32)
}

// The iwlwifi thermal zone is not linked to its device, so it is only used when
// there is exactly one iwlwifi adapter and one iwlwifi zone
fn iwlwifi_milli_c(base: &str) -> Option<i64> {
    let is_iwlwifi =
        |driver: &Path| std::fs::read_link(driver).is_ok_and(|d| d.ends_with("iwlwifi"));

    if !is_iwlwifi(&Path::new(base).join("device/driver"))
        || paths("/sys/class/ieee80211/*/device/driver")
            .filter(|p| is_iwlwifi(p))
            .count()
            != 1
    {
        return None;
    }

    let mut zones = paths("/sys/class/thermal/thermal_zone*").filter(|zone| {
        std::fs::read_to_string(zone.join("type")).is_ok_and(|t| t.starts_with("iwlwifi_"))
    });
    let zone = zones.next()?;
    if zones.next().is_some() {
        return None;
    }

    read_milli_c(&zone.join("temp"))
}

fn max_milli_c(pattern: &str) -> Option<i64> {
    paths(pattern).filter_map(|p| read_milli_c(&p)).max()
}

fn paths(pattern: &str) -> impl Iterator<Item = std::path::PathBuf> {
    glob(pattern).into_iter().flatten().filter_map(Result::ok)
}

// Reading fails routinely (e.g. while an adapter is down), so errors are not logged
fn read_milli_c(path: &Path) -> Option<i64> {
    std::fs::read_to_string(path)
        .ok()?
        .trim()
        .parse()
        .ok()
        .filter(|v| *v > THERMAL_TEMP_INVALID)
}
