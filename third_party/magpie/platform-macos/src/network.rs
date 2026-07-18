/* src/network.rs
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
use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Instant;

use magpie_platform::network::{
    Connection, ConnectionKind, ConnectionState, WirelessConnection,
};

struct PrevStats {
    tx: u64,
    rx: u64,
    ts: Instant,
}

pub struct NetworkCache {
    connections: HashMap<String, Connection>,
    prev: HashMap<String, PrevStats>,
    wifi_cache: HashMap<String, WirelessConnection>,
    wifi_cache_ts: Option<Instant>,
}

impl magpie_platform::network::NetworkCache for NetworkCache {
    fn new() -> Self {
        Self {
            connections: HashMap::new(),
            prev: HashMap::new(),
            wifi_cache: HashMap::new(),
            wifi_cache_ts: None,
        }
    }

    fn refresh(&mut self) {
        let hw_ports = list_hardware_ports();
        if hw_ports.is_empty() {
            self.connections.clear();
            return;
        }

        let stats = read_netstat();
        let now = Instant::now();
        let mut next = HashMap::with_capacity(hw_ports.len());

        let mut need_wifi = false;
        for (_, port_name, _) in &hw_ports {
            if is_wireless_port(port_name) {
                need_wifi = true;
                break;
            }
        }
        if need_wifi {
            let stale = self
                .wifi_cache_ts
                .map(|ts| ts.elapsed().as_secs() >= 5)
                .unwrap_or(true);
            if stale {
                self.wifi_cache = load_wifi_cache();
                self.wifi_cache_ts = Some(Instant::now());
            }
        }

        for (if_name, port_name, hw_addr) in hw_ports {
            let (tx, rx) = stats.get(&if_name).copied().unwrap_or((0, 0));
            let (tx_rate, rx_rate) = if let Some(prev) = self.prev.get(&if_name) {
                let elapsed = now.duration_since(prev.ts).as_secs_f64().max(0.001);
                (
                    tx.saturating_sub(prev.tx) as f64 / elapsed,
                    rx.saturating_sub(prev.rx) as f64 / elapsed,
                )
            } else {
                (0.0, 0.0)
            };
            self.prev.insert(
                if_name.clone(),
                PrevStats {
                    tx,
                    rx,
                    ts: now,
                },
            );

            let kind = connection_kind(&port_name);
            let wireless_connection = if kind == ConnectionKind::Wireless {
                self.wifi_cache.get(&if_name).cloned()
            } else {
                None
            };

            let (ipv4_address, ipv6_address) = if_addresses(&if_name);
            let state = if ipv4_address.is_some() || ipv6_address.is_some() {
                ConnectionState::Connected
            } else {
                ConnectionState::Disconnected
            };

            next.insert(
                if_name.clone(),
                Connection {
                    id: if_name,
                    device_name: Some(port_name),
                    kind: kind as i32,
                    wireless_connection,
                    hw_address: hw_addr.unwrap_or_default(),
                    tx_rate_bytes_ps: tx_rate as f32,
                    tx_total_bytes: tx,
                    rx_rate_bytes_ps: rx_rate as f32,
                    rx_total_bytes: rx,
                    max_speed_bytes_ps: None,
                    ipv4_address,
                    ipv6_address,
                    state: state as i32,
                },
            );
        }

        self.connections = next;
    }

    fn cached_entries(&self) -> &HashMap<String, Connection> {
        &self.connections
    }
}

fn is_wireless_port(port_name: &str) -> bool {
    port_name.contains("Wi-Fi")
        || port_name.contains("AirPort")
        || port_name.contains("Wireless")
}

fn connection_kind(port_name: &str) -> ConnectionKind {
    if is_wireless_port(port_name) {
        ConnectionKind::Wireless
    } else if port_name.contains("Thunderbolt") || port_name.contains("Bridge") {
        ConnectionKind::Bridge
    } else if port_name.contains("Bluetooth") {
        ConnectionKind::Bluetooth
    } else if port_name.contains("VPN") || port_name.contains("utun") {
        ConnectionKind::Vpn
    } else {
        ConnectionKind::Wired
    }
}

/// Returns (device, port name, hw address string).
fn list_hardware_ports() -> Vec<(String, String, Option<String>)> {
    let output = std::process::Command::new("/usr/sbin/networksetup")
        .args(["-listallhardwareports"])
        .output();
    let text = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => return vec![],
    };

    let mut result = vec![];
    let mut current_port: Option<String> = None;
    let mut current_device: Option<String> = None;

    for line in text.lines() {
        if let Some(port) = line.strip_prefix("Hardware Port: ") {
            current_port = Some(port.trim().to_string());
            current_device = None;
        } else if let Some(dev) = line.strip_prefix("Device: ") {
            current_device = Some(dev.trim().to_string());
        } else if line.starts_with("Ethernet Address: ") {
            if let (Some(port), Some(dev)) = (current_port.take(), current_device.take()) {
                let addr_str = line.trim_start_matches("Ethernet Address: ").trim();
                let hw_addr = parse_mac_address(addr_str).map(|b| {
                    format!(
                        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                        b[0], b[1], b[2], b[3], b[4], b[5]
                    )
                });
                result.push((dev, port, hw_addr));
            }
        }
    }

    result
}

/// Map of interface -> (tx_bytes, rx_bytes).
fn read_netstat() -> HashMap<String, (u64, u64)> {
    let mut map = HashMap::new();
    let output = std::process::Command::new("/usr/sbin/netstat")
        .args(["-ibn"])
        .output();
    if let Ok(o) = output {
        let s = String::from_utf8_lossy(&o.stdout);
        for line in s.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 10 {
                continue;
            }
            let name = parts[0].to_string();
            let rx: u64 = parts[6].parse().unwrap_or(0);
            let tx: u64 = parts[9].parse().unwrap_or(0);
            map.entry(name).or_insert((tx, rx));
        }
    }
    map
}

fn load_wifi_cache() -> HashMap<String, WirelessConnection> {
    let mut result = HashMap::new();

    let sp_out = std::process::Command::new("/usr/sbin/system_profiler")
        .args(["SPAirPortDataType"])
        .output();
    let text = match sp_out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => return result,
    };

    let mut current_iface: Option<String> = None;
    let mut in_current_net = false;
    let mut ssid: Option<String> = None;
    let mut channel_str: Option<String> = None;
    let mut signal_noise: Option<String> = None;
    let mut tx_rate: Option<u32> = None;

    let flush = |iface: &Option<String>,
                 ssid: &mut Option<String>,
                 channel_str: &mut Option<String>,
                 signal_noise: &mut Option<String>,
                 tx_rate: &mut Option<u32>,
                 result: &mut HashMap<String, WirelessConnection>| {
        if let Some(ref name) = iface {
            let frequency_mhz = channel_str.as_deref().and_then(parse_channel_to_mhz);
            let signal_strength_percent = signal_noise
                .as_deref()
                .and_then(parse_signal_percent)
                .map(|p| p as u32);
            let bitrate_kbps = tx_rate.map(|r| r * 1000);
            result.insert(
                name.clone(),
                WirelessConnection {
                    ssid: ssid.take(),
                    frequency_mhz,
                    bitrate_kbps,
                    signal_strength_percent,
                },
            );
        }
        *channel_str = None;
        *signal_noise = None;
        *tx_rate = None;
    };

    for line in text.lines() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start().len();

        if indent == 8 && trimmed.ends_with(':') && !trimmed.starts_with('<') {
            flush(
                &current_iface,
                &mut ssid,
                &mut channel_str,
                &mut signal_noise,
                &mut tx_rate,
                &mut result,
            );
            current_iface = Some(trimmed.trim_end_matches(':').to_string());
            in_current_net = false;
            ssid = None;
        } else if trimmed == "Current Network Information:" {
            in_current_net = true;
        } else if trimmed == "Other Local Wi-Fi Networks:" {
            in_current_net = false;
        } else if in_current_net {
            if indent == 12 && trimmed.ends_with(':') && !trimmed.starts_with('<') {
                ssid = Some(trimmed.trim_end_matches(':').to_string());
            } else if let Some(v) = trimmed.strip_prefix("Channel: ") {
                channel_str = Some(v.to_string());
            } else if let Some(v) = trimmed.strip_prefix("Signal / Noise: ") {
                signal_noise = Some(v.to_string());
            } else if let Some(v) = trimmed.strip_prefix("Transmit Rate: ") {
                tx_rate = v.parse().ok();
            }
        }
    }
    flush(
        &current_iface,
        &mut ssid,
        &mut channel_str,
        &mut signal_noise,
        &mut tx_rate,
        &mut result,
    );

    // Fallback SSID via ipconfig when system_profiler redacts it.
    for (if_name, info) in result.iter_mut() {
        if info.ssid.is_none() {
            let out = std::process::Command::new("/usr/sbin/ipconfig")
                .args(["getsummary", if_name])
                .output();
            if let Ok(o) = out {
                let s = String::from_utf8_lossy(&o.stdout);
                for line in s.lines() {
                    let t = line.trim();
                    if let Some(v) = t.strip_prefix("SSID : ") {
                        let v = v.trim();
                        if !v.is_empty() && v != "<redacted>" {
                            info.ssid = Some(v.to_string());
                        }
                        break;
                    }
                }
            }
        }
    }

    result
}

fn if_addresses(if_name: &str) -> (Option<String>, Option<String>) {
    let mut ip4: Option<String> = None;
    let mut ip6: Option<String> = None;
    let mut ip6_link_local: Option<String> = None;

    unsafe {
        let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifap) != 0 {
            return (None, None);
        }
        let mut cursor = ifap;
        while !cursor.is_null() {
            let ifa = &*cursor;
            if !ifa.ifa_name.is_null() {
                let ifa_name = std::ffi::CStr::from_ptr(ifa.ifa_name).to_string_lossy();
                if ifa_name == if_name && !ifa.ifa_addr.is_null() {
                    let family = (*ifa.ifa_addr).sa_family as i32;
                    if family == libc::AF_INET && ip4.is_none() {
                        let sin = ifa.ifa_addr as *const libc::sockaddr_in;
                        // s_addr is network byte order.
                        let addr = u32::from_be((*sin).sin_addr.s_addr);
                        ip4 = Some(Ipv4Addr::from(addr).to_string());
                    } else if family == libc::AF_INET6 {
                        let sin6 = ifa.ifa_addr as *const libc::sockaddr_in6;
                        let b = (*sin6).sin6_addr.s6_addr;
                        let addr = Ipv6Addr::from(b);
                        if addr.is_unicast_link_local() {
                            if ip6_link_local.is_none() {
                                ip6_link_local = Some(addr.to_string());
                            }
                        } else if ip6.is_none() {
                            ip6 = Some(addr.to_string());
                        }
                    }
                }
            }
            cursor = ifa.ifa_next;
        }
        libc::freeifaddrs(ifap);
    }

    (ip4, ip6.or(ip6_link_local))
}

fn parse_mac_address(s: &str) -> Option<[u8; 6]> {
    let parts: Vec<u8> = s
        .split(':')
        .filter_map(|x| u8::from_str_radix(x, 16).ok())
        .collect();
    if parts.len() == 6 {
        Some([parts[0], parts[1], parts[2], parts[3], parts[4], parts[5]])
    } else {
        None
    }
}

fn parse_channel_to_mhz(channel_str: &str) -> Option<u32> {
    let ch: u32 = channel_str.split_whitespace().next()?.parse().ok()?;
    if (1..=14).contains(&ch) {
        Some(2407 + ch * 5)
    } else if (36..=64).contains(&ch) || (100..=177).contains(&ch) {
        Some(5000 + ch * 5)
    } else {
        None
    }
}

fn parse_signal_percent(signal_noise: &str) -> Option<u8> {
    let rssi: i32 = signal_noise
        .split('/')
        .next()?
        .trim()
        .trim_end_matches(" dBm")
        .trim()
        .parse()
        .ok()?;
    let pct = ((rssi + 100).clamp(0, 70) * 100 / 70) as u8;
    Some(pct)
}
