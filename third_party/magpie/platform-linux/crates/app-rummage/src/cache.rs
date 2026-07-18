use std::cell::RefCell;
use std::collections::HashMap;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use nix::errno::Errno;
use nix::sys::inotify::{AddWatchFlags, Inotify, WatchDescriptor};

use crate::ApplicationEntry;

pub(crate) struct AppsCache {
    pub(crate) apps: HashMap<Rc<str>, ApplicationEntry>,
    pub(crate) inotify: Inotify,
    pub(crate) watched: HashMap<WatchDescriptor, PathBuf>,
}

thread_local! {
    pub(crate) static APPS_CACHE: RefCell<Option<AppsCache>> = const { RefCell::new(None) };
}

/// Drains all pending inotify events. Returns `true` if any of them indicate that
/// the cached desktop-file set may be out of date.
pub(crate) fn drain_inotify_events(cache: &mut AppsCache) -> bool {
    let mut dirty = false;
    loop {
        match cache.inotify.read_events() {
            Ok(events) => {
                for ev in events {
                    // Queue overflow means we may have missed events; rebuild conservatively.
                    if ev.mask.contains(AddWatchFlags::IN_Q_OVERFLOW) {
                        dirty = true;
                        continue;
                    }
                    // The kernel auto-removes a watch when its target is gone (dir
                    // deleted/unmounted). Drop our bookkeeping so ensure_watches can
                    // re-add it if the directory comes back.
                    if ev.mask.contains(AddWatchFlags::IN_IGNORED) {
                        cache.watched.remove(&ev.wd);
                        dirty = true;
                        continue;
                    }
                    // We only care about events naming a `.desktop` entry. Other dotfiles
                    // and editor swap files (e.g. `.foo.desktop.swp`) shouldn't churn the cache.
                    if let Some(name) = ev.name.as_ref() {
                        if name.as_bytes().ends_with(b".desktop") {
                            dirty = true;
                        }
                    }
                }
            }
            Err(Errno::EAGAIN) => break,
            Err(e) => {
                log::warn!("inotify read_events failed ({e}); invalidating apps cache");
                dirty = true;
                break;
            }
        }
    }
    dirty
}

/// Adds inotify watches for every `applications/` directory under the given data
/// dirs that exists but isn't already watched. Watches are kept across calls; this
/// only ever adds new ones (existing watches stay in `cache.watched`).
pub(crate) fn ensure_watches(cache: &mut AppsCache, xdg_data_dirs: &[String]) {
    const FLAGS: AddWatchFlags = AddWatchFlags::IN_CREATE
        .union(AddWatchFlags::IN_DELETE)
        .union(AddWatchFlags::IN_MODIFY)
        .union(AddWatchFlags::IN_MOVED_FROM)
        .union(AddWatchFlags::IN_MOVED_TO)
        .union(AddWatchFlags::IN_CLOSE_WRITE)
        .union(AddWatchFlags::IN_ONLYDIR);

    for dir in xdg_data_dirs {
        let path = Path::new(dir).join("applications");
        if !path.exists() {
            continue;
        }
        if cache.watched.values().any(|p| p == &path) {
            continue;
        }
        match cache.inotify.add_watch(&path, FLAGS) {
            Ok(wd) => {
                cache.watched.insert(wd, path);
            }
            Err(e) => {
                log::warn!("inotify add_watch failed for {}: {}", path.display(), e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::installed_apps_impl;
    use nix::sys::inotify::InitFlags;

    #[test]
    fn cache_invalidates_on_desktop_file_change() {
        // Build a cache rooted at a temp xdg_data_dir, write a desktop file, observe
        // the cache picks it up; then modify it and observe the cache reports dirty
        // through the inotify-event drain.
        let test_root =
            std::env::temp_dir().join(format!("app_rummage_cache_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&test_root);
        let apps_dir = test_root.join("applications");
        std::fs::create_dir_all(&apps_dir).unwrap();

        let dirs = vec![test_root.to_string_lossy().into_owned()];

        let inotify = Inotify::init(InitFlags::IN_NONBLOCK | InitFlags::IN_CLOEXEC).unwrap();
        let initial = installed_apps_impl(&dirs, &[]);
        let mut cache = AppsCache {
            apps: initial,
            inotify,
            watched: HashMap::new(),
        };
        ensure_watches(&mut cache, &dirs);
        assert_eq!(cache.watched.len(), 1, "should have watched applications/");
        assert!(cache.apps.is_empty(), "no desktop files yet");

        // Create a desktop file. This must produce a CREATE (and CLOSE_WRITE) event
        // for `*.desktop`, which drain_inotify_events should classify as dirty.
        let desktop_path = apps_dir.join("org.example.New.desktop");
        std::fs::write(
            &desktop_path,
            "[Desktop Entry]\nType=Application\nName=New\nExec=/usr/bin/true\n",
        )
        .unwrap();

        assert!(
            drain_inotify_events(&mut cache),
            "creating a .desktop file must dirty the cache",
        );

        // Re-scan and confirm the new entry shows up.
        cache.apps = installed_apps_impl(&dirs, &[]);
        assert!(cache.apps.contains_key("org.example.New"));

        // A change to a non-`.desktop` sibling must NOT dirty the cache.
        std::fs::write(apps_dir.join("README.txt"), b"ignore me").unwrap();
        assert!(
            !drain_inotify_events(&mut cache),
            "non-.desktop file changes must not dirty the cache",
        );

        let _ = std::fs::remove_dir_all(&test_root);
    }
}
