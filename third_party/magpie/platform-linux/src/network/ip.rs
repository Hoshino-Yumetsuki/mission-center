/* src/network/ip.rs
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

use tokio::runtime::Handle;
use zbus::zvariant::{Array, Dict, OwnedObjectPath};
use zbus::Proxy;

use crate::sync;

pub fn ipv4_address(
    if_name: &str,
    rt: &Handle,
    bus: &zbus::Connection,
    device_proxy: &Proxy,
) -> Option<String> {
    ip_address(
        if_name,
        rt,
        bus,
        device_proxy,
        "Ip4Config",
        "org.freedesktop.NetworkManager.IP4Config",
    )
}

pub fn ipv6_address(
    if_name: &str,
    rt: &Handle,
    bus: &zbus::Connection,
    device_proxy: &Proxy,
) -> Option<String> {
    ip_address(
        if_name,
        rt,
        bus,
        device_proxy,
        "Ip6Config",
        "org.freedesktop.NetworkManager.IP6Config",
    )
}

fn ip_address(
    if_name: &str,
    rt: &Handle,
    bus: &zbus::Connection,
    device_proxy: &Proxy,
    config_property: &str,
    config_interface: &'static str,
) -> Option<String> {
    let config_path = match sync!(
        rt,
        device_proxy.get_property::<OwnedObjectPath>(config_property)
    ) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Failed to get {config_property} for {if_name}: {e}");
            return None;
        }
    };

    // NetworkManager returns "/" when the device doesn't have an IP address
    if config_path.as_str() == "/" {
        return None;
    }

    let config_proxy = match sync!(
        rt,
        Proxy::new(
            bus,
            "org.freedesktop.NetworkManager",
            config_path,
            config_interface
        )
    ) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Failed to get {config_interface} proxy for {if_name}: {e}");
            return None;
        }
    };

    match sync!(rt, config_proxy.get_property::<Array>("AddressData")) {
        Ok(arr) => {
            for address in arr.inner() {
                let Ok(address) = address.downcast_ref::<Dict>() else {
                    continue;
                };

                for (key, value) in address.iter() {
                    let Ok(key) = key.downcast_ref::<&str>() else {
                        continue;
                    };

                    if key == "address" {
                        let Ok(value) = value.downcast_ref::<&str>() else {
                            continue;
                        };

                        return Some(value.to_owned());
                    }
                }
            }
        }
        Err(e) => log::warn!("Failed to get {config_interface} AddressData for {if_name}: {e}"),
    }

    None
}
