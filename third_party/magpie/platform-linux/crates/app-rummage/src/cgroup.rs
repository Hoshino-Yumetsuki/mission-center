use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::num::NonZeroU32;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::rc::Rc;
use std::sync::OnceLock;

use regex::Regex;

use crate::ApplicationEntry;

pub(crate) fn find_pids_for_cgroup(cgroup_path: &Path) -> Vec<NonZeroU32> {
    let mut result = vec![];
    walk_cgroup_tree(cgroup_path, &mut result);
    result.sort_unstable();
    result
}

fn walk_cgroup_tree(cgroup_path: &Path, result: &mut Vec<NonZeroU32>) {
    let procs = cgroup_path.join("cgroup.procs");
    if let Ok(file) = std::fs::File::open(procs) {
        for line in BufReader::new(file).lines() {
            if let Ok(pid_str) = line.as_ref().map(|l| l.trim()) {
                if let Ok(pid) = pid_str.parse::<u32>() {
                    if let Some(pid) = NonZeroU32::new(pid) {
                        result.push(pid);
                    }
                }
            }
        }
    }

    let cgroup_entries = match cgroup_path.read_dir() {
        Ok(r) => r,
        Err(_) => {
            return;
        }
    };

    for entry in cgroup_entries.filter_map(|e| e.ok()) {
        if let Ok(kind) = entry.file_type() {
            if kind.is_dir() {
                walk_cgroup_tree(&entry.path(), result);
            }
        }
    }
}

pub(crate) fn app_id(path: &Path) -> Option<Rc<str>> {
    // https://systemd.io/DESKTOP_ENVIRONMENTS/#xdg-standardization-for-applications

    let dir_name = path.file_name()?.to_string_lossy();

    if dir_name.starts_with("snap.") {
        let mut app_id = String::new();

        // snaps cgroups names don't conform to the suggested XDG standard they, instead, look like:
        // snap.<appname1>.<appname2>-<UUID>.scope
        //
        // The app id is the concatenation of appname1 and appname2 separated by an underscore
        for part in dir_name.split('.').skip(1) {
            if part == "scope" {
                break;
            }

            if !app_id.is_empty() {
                app_id.push('_');
            }

            // Try to skip over the uuid part by counting the number of '-' characters
            // from the end of the part
            let mut uuid_pos = part.len();
            let mut counter = 0;
            for (i, c) in part.as_bytes().iter().enumerate().rev() {
                if *c == b'-' {
                    counter += 1;
                }

                if counter == 5 {
                    uuid_pos = i;
                    break;
                }
            }
            app_id.push_str(&part[..uuid_pos]);
        }

        Some(Rc::from(app_id))
    } else {
        // Matches the systemd/XDG unit naming scheme:
        //   app[s]-[<launcher>-]<id>[@<random>].{service,slice}
        //   app[s]-[<launcher>-]<id>-<random>.scope
        //   dbus-[<launcher>-]<id>[@<random>].service
        //   flatpak-[<launcher>-]<id>...
        // Based on ksysguard's cgroup parsing code
        static APP_UNIT_RE: OnceLock<Regex> = OnceLock::new();
        let re = APP_UNIT_RE.get_or_init(|| {
            Regex::new(
                r"^(?:apps|app|flatpak|dbus)-(?:[^-]*-)?(?:([^-]+)-[^-]*\.scope|([^@]+?)(?:@[^@]*)?\.(?:service|slice))$",
            )
            .expect("invalid cgroup app-unit regex")
        });

        let caps = re.captures(&dir_name)?;
        let raw = caps.get(1).or_else(|| caps.get(2))?.as_str();
        Some(Rc::from(unescape_cgroup_name(raw)))
    }
}

/// Decode systemd-style `\xZZ` byte escapes.
fn unescape_cgroup_name(s: &str) -> String {
    fn hex_digit(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }

    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && bytes.get(i + 1) == Some(&b'x') {
            if let (Some(h), Some(l)) = (
                bytes.get(i + 2).copied().and_then(hex_digit),
                bytes.get(i + 3).copied().and_then(hex_digit),
            ) {
                out.push((h << 4) | l);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_owned())
}

pub(crate) fn running_xdg_conformant_apps(
    available_apps: &HashMap<Rc<str>, ApplicationEntry>,
) -> HashMap<Rc<str>, (&ApplicationEntry, Vec<NonZeroU32>)> {
    // We only show running apps for the current user
    let uid = nix::unistd::getuid();
    let app_slice_dir =
        format!("/sys/fs/cgroup/user.slice/user-{uid}.slice/user@{uid}.service/app.slice");
    let app_slice_dir = match Path::new(&app_slice_dir).read_dir() {
        Ok(r) => r,
        Err(e) => {
            log::warn!(
                "Error reading cgroup information from {}: {}",
                app_slice_dir,
                e
            );
            return HashMap::new();
        }
    };

    let mut result: HashMap<Rc<str>, (&ApplicationEntry, Vec<NonZeroU32>)> = HashMap::new();
    result.reserve(available_apps.len());

    for entry in app_slice_dir.filter_map(|e| e.ok()).filter(|e| {
        let file_name = e.file_name();
        let file_name = file_name.as_bytes();
        file_name.ends_with(b".slice")
            || file_name.ends_with(b".scope")
            || file_name.ends_with(b".service")
    }) {
        let path = entry.path();

        if let Some(app_id) = app_id(&path) {
            let mut pids = find_pids_for_cgroup(&path);
            if !pids.is_empty() {
                if let Some(app) = available_apps.get(&app_id) {
                    if let Some((_, existing_pids)) = result.get_mut(&app.id) {
                        existing_pids.extend(pids);
                        existing_pids.sort_unstable();
                    } else {
                        pids.sort_unstable();
                        result.insert(app.id.clone(), (app, pids));
                    }
                }
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id_of(unit: &str) -> Option<String> {
        app_id(Path::new(unit)).map(|rc| rc.as_ref().to_owned())
    }

    // App ID parsing tests. These are mostly based on the test vectors in libksysguard's cgroup test.

    #[test]
    fn app_id_ksysguard_vectors() {
        // Straight port of libksysguard's autotests/cgrouptest.cpp test vectors.
        assert_eq!(
            id_of("app-gnome-org.gnome.Evince@12345.service").as_deref(),
            Some("org.gnome.Evince"),
        );
        assert_eq!(
            id_of("app-flatpak-org.telegram.desktop@12345.service").as_deref(),
            Some("org.telegram.desktop"),
        );
        assert_eq!(
            id_of("app-org.kde.okular@12345.service").as_deref(),
            Some("org.kde.okular"),
        );
        assert_eq!(
            id_of("app-KDE-org.kde.okular.service").as_deref(),
            Some("org.kde.okular"),
        );
        assert_eq!(
            id_of("app-org.kde.amarok.service").as_deref(),
            Some("org.kde.amarok"),
        );
        assert_eq!(
            id_of("app-gnome-org.gnome.Evince-12345.scope").as_deref(),
            Some("org.gnome.Evince"),
        );
        assert_eq!(
            id_of("app-org.gnome.Evince-12345.scope").as_deref(),
            Some("org.gnome.Evince"),
        );
        assert_eq!(
            id_of("dbus-:1.2-org.kde.kdeconnect@0.service").as_deref(),
            Some("org.kde.kdeconnect"),
        );
    }

    #[test]
    fn app_id_autostart_service() {
        // The autostart case from libksysguard: the `@autostart` part is just a random-id slot.
        assert_eq!(
            id_of("app-org.kde.korgac@autostart.service").as_deref(),
            Some("org.kde.korgac"),
        );
    }

    #[test]
    fn app_id_slice_suffix() {
        // GNOME Terminal and similar apps live in their own .slice rather than a .scope/.service.
        assert_eq!(
            id_of("app-org.gnome.Terminal@12345.slice").as_deref(),
            Some("org.gnome.Terminal"),
        );
        assert_eq!(
            id_of("app-org.gnome.Terminal.slice").as_deref(),
            Some("org.gnome.Terminal"),
        );
    }

    #[test]
    fn app_id_dash_escape() {
        // Desktop IDs containing a literal `-` are escaped by systemd as \x2d.
        assert_eq!(
            id_of("app-org.foo\\x2dbar-12345.scope").as_deref(),
            Some("org.foo-bar"),
        );
        assert_eq!(
            id_of("app-org.foo\\x2dbar.service").as_deref(),
            Some("org.foo-bar"),
        );
        assert_eq!(
            id_of("app-gnome-org.foo\\x2dbar-12345.scope").as_deref(),
            Some("org.foo-bar"),
        );
    }

    #[test]
    fn app_id_apps_prefix() {
        // The spec historically used `apps-` before switching to `app-`.
        assert_eq!(
            id_of("apps-org.kde.okular.service").as_deref(),
            Some("org.kde.okular"),
        );
    }

    #[test]
    fn app_id_rejects_non_application_units() {
        assert_eq!(id_of("user.slice"), None);
        assert_eq!(id_of("session.slice"), None);
        assert_eq!(id_of("background.slice"), None);
        assert_eq!(id_of("dbus.service"), None);
        assert_eq!(id_of("random-thing.scope"), None);
        assert_eq!(id_of(""), None);
    }

    #[test]
    fn unescape_known_chars() {
        assert_eq!(unescape_cgroup_name("org.foo\\x2dbar"), "org.foo-bar");
        assert_eq!(unescape_cgroup_name("hello\\x20world"), "hello world");
        assert_eq!(unescape_cgroup_name("\\x40.service"), "@.service");
        assert_eq!(unescape_cgroup_name("a\\x2db\\x2dc"), "a-b-c");
    }

    #[test]
    fn unescape_case_insensitive_hex() {
        assert_eq!(unescape_cgroup_name("\\x41\\x42"), "AB");
        assert_eq!(unescape_cgroup_name("\\x2D"), "-");
        assert_eq!(unescape_cgroup_name("\\x2d"), "-");
    }

    #[test]
    fn unescape_multibyte_utf8() {
        // é = U+00E9, UTF-8 = 0xC3 0xA9
        assert_eq!(unescape_cgroup_name("\\xc3\\xa9"), "é");
        // 中 = U+4E2D, UTF-8 = 0xE4 0xB8 0xAD
        assert_eq!(unescape_cgroup_name("\\xe4\\xb8\\xad"), "中");
        assert_eq!(unescape_cgroup_name("caf\\xc3\\xa9"), "café");
    }

    #[test]
    fn unescape_invalid_escape_left_literal() {
        // Non-hex digit: keep the `\` literal and move on.
        assert_eq!(unescape_cgroup_name("\\xzz"), "\\xzz");
        assert_eq!(unescape_cgroup_name("\\x2"), "\\x2");
        assert_eq!(unescape_cgroup_name("\\x"), "\\x");
        // Lone backslash.
        assert_eq!(unescape_cgroup_name("foo\\bar"), "foo\\bar");
    }

    #[test]
    fn unescape_invalid_utf8_falls_back() {
        // 0xFF alone is not a valid UTF-8 start byte: return input unchanged.
        assert_eq!(unescape_cgroup_name("\\xff"), "\\xff");
    }

    #[test]
    fn unescape_empty_and_plain() {
        assert_eq!(unescape_cgroup_name(""), "");
        assert_eq!(unescape_cgroup_name("org.kde.okular"), "org.kde.okular");
    }

    #[test]
    fn app_id_unescapes_non_dash() {
        assert_eq!(id_of("app-foo\\x20bar.service").as_deref(), Some("foo bar"),);
        assert_eq!(
            id_of("app-org.foo\\x40bar@12345.service").as_deref(),
            Some("org.foo@bar"),
        );
    }

    #[test]
    fn app_id_snap_unchanged() {
        assert_eq!(
            id_of("snap.firefox.firefox.scope").as_deref(),
            Some("firefox_firefox"),
        );
    }

    #[test]
    fn test_find_pids_for_cgroup() {
        let result = find_pids_for_cgroup(Path::new("/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service/app.slice/app-org.gnome.Terminal.slice"));
        dbg!(&result);
    }

    #[test]
    fn test_running_applications_xdg() {
        let available_apps = crate::installed_apps();
        let result = running_xdg_conformant_apps(&available_apps);
        dbg!(&result);
    }
}
