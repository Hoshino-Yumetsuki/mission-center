/* src/network/utils.rs
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

use std::fmt::Write;
use std::path::Path;

use arrayvec::ArrayString;

use magpie_platform::network::ConnectionKind;

use crate::util::{parse_u64, read_bytes_fixed};

const NM_DEVICE_TYPE_ETHERNET: u32 = 1;
const NM_DEVICE_TYPE_WIFI: u32 = 2;
const NM_DEVICE_TYPE_BT: u32 = 5;
const NM_DEVICE_TYPE_MODEM: u32 = 8;
const NM_DEVICE_TYPE_INFINIBAND: u32 = 9;
const NM_DEVICE_TYPE_BRIDGE: u32 = 13;
const NM_DEVICE_TYPE_TUN: u32 = 16;
const NM_DEVICE_TYPE_MACVLAN: u32 = 18;
const NM_DEVICE_TYPE_VXLAN: u32 = 19;
const NM_DEVICE_TYPE_VETH: u32 = 20;
const NM_DEVICE_TYPE_DUMMY: u32 = 22;
const NM_DEVICE_TYPE_WIREGUARD: u32 = 29;
const NM_DEVICE_TYPE_IPVLAN: u32 = 34;

pub fn connection_kind(nm_device_type: Option<u32>, if_name: &str) -> ConnectionKind {
    let kind_from_name = connection_kind_from_name(if_name);

    match nm_device_type {
        Some(NM_DEVICE_TYPE_ETHERNET) => ConnectionKind::Wired,
        Some(NM_DEVICE_TYPE_WIFI) => ConnectionKind::Wireless,
        Some(NM_DEVICE_TYPE_BT) => ConnectionKind::Bluetooth,
        Some(NM_DEVICE_TYPE_MODEM) => ConnectionKind::Wwan,
        Some(NM_DEVICE_TYPE_INFINIBAND) => ConnectionKind::InfiniBand,
        Some(NM_DEVICE_TYPE_BRIDGE) => match kind_from_name {
            ConnectionKind::Docker | ConnectionKind::Multipass => kind_from_name,
            _ => ConnectionKind::Bridge,
        },
        Some(NM_DEVICE_TYPE_TUN) => match kind_from_name {
            ConnectionKind::Other => tun_kind(if_name),
            _ => kind_from_name,
        },
        Some(
            NM_DEVICE_TYPE_MACVLAN
            | NM_DEVICE_TYPE_VXLAN
            | NM_DEVICE_TYPE_VETH
            | NM_DEVICE_TYPE_DUMMY
            | NM_DEVICE_TYPE_IPVLAN,
        ) => ConnectionKind::Virtual,
        Some(NM_DEVICE_TYPE_WIREGUARD) => ConnectionKind::Vpn,
        _ => kind_from_name,
    }
}

fn tun_kind(if_name: &str) -> ConnectionKind {
    let mut path: ArrayString<256> = ArrayString::new();
    let _ = write!(&mut path, "/sys/class/net/{if_name}/tun_flags");

    let mut buffer = [0; 32];
    let flags = read_bytes_fixed(path.as_str(), "TUN flags", &mut buffer)
        .and_then(|_| parse_u64(&buffer, 16, "TUN flags"));

    match flags {
        // TAP is usually used for virtual network interfaces
        Some(f) if f & libc::IFF_TAP as u64 != 0 => ConnectionKind::Virtual,
        // TUN is usually used for VPNs
        Some(f) if f & libc::IFF_TUN as u64 != 0 => ConnectionKind::Vpn,
        _ => ConnectionKind::Other,
    }
}

fn connection_kind_from_name(if_name: &str) -> ConnectionKind {
    if if_name.starts_with("bn") {
        ConnectionKind::Bluetooth
    } else if if_name.starts_with("br") || if_name.starts_with("virbr") {
        ConnectionKind::Bridge
    } else if if_name.starts_with("docker") {
        ConnectionKind::Docker
    } else if if_name.starts_with("eth") || if_name.starts_with("en") {
        ConnectionKind::Wired
    } else if if_name.starts_with("ib") {
        ConnectionKind::InfiniBand
    } else if if_name.starts_with("mp") {
        ConnectionKind::Multipass
    } else if if_name.starts_with("veth") {
        ConnectionKind::Virtual
    } else if if_name.starts_with("vpn") || if_name.starts_with("wg") {
        ConnectionKind::Vpn
    } else if if_name.starts_with("wl") || if_name.starts_with("ww") {
        ConnectionKind::Wireless
    } else if if_name.starts_with("mlan") {
        let path = Path::new("/sys/class/net").join(if_name).join("wireless");
        if path.exists() {
            ConnectionKind::Wireless
        } else {
            ConnectionKind::Other
        }
    } else {
        ConnectionKind::Other
    }
}
