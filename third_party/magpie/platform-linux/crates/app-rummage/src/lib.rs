use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::OnceLock;

use nix::sys::inotify::{InitFlags, Inotify};

use cache::{drain_inotify_events, ensure_watches, AppsCache, APPS_CACHE};
use cgroup::running_xdg_conformant_apps;
use desktop::installed_apps_impl;
use matching::running_apps_by_process;

mod cache;
mod cgroup;
mod desktop;
mod env;
mod exec;
mod matching;

fn app_id_replacement() -> &'static HashMap<&'static str, &'static str> {
    static APP_ID_REPLACEMENT: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();

    APP_ID_REPLACEMENT
        .get_or_init(|| HashMap::from([("gnome-system-monitor-kde", "org.gnome.SystemMonitor")]))
}

fn normalize_app_id(id: &str) -> &str {
    if let Some(real_id) = app_id_replacement().get(id) {
        real_id
    } else {
        id
    }
}

/// A running process
///
/// Used to identify running applications. ApplicationEntry's exec field is compared against the
/// process's executable path to determine if the process is an application.
pub trait Process {
    /// The process's PID
    fn pid(&self) -> NonZeroU32;
    /// The process's executable path
    ///
    /// It is expected that the path is canonical
    fn executable_path(&self) -> Option<PathBuf>;
    /// The process's name
    ///
    /// Essentially the name of the process as seen in `ps`
    fn name(&self) -> &str;
    /// The process's full argv as read from `/proc/<pid>/cmdline`
    ///
    /// Used to disambiguate processes that share an executable between multiple
    /// applications (e.g. Chromium-based PWAs, custom launcher shortcuts for the
    /// same binary with different args). Returning an empty slice disables argv
    /// disambiguation for this process; matching degrades to "binary path only",
    /// so processes in shared-binary buckets will be dropped rather than attributed
    /// arbitrarily.
    fn cmdline(&self) -> &[String] {
        &[]
    }
}

/// And data structure representing an application
///
/// It contains a subset of the fields from an XDG desktop entry file
#[derive(Clone, Debug)]
pub struct ApplicationEntry {
    /// The application's id
    ///
    /// The filename of the desktop entry file without the `.desktop` extension and is used
    /// to uniquely identify an application
    pub id: Rc<str>,
    /// The application's name
    ///
    /// The name that is displayed to the user in the application menu, taskbar, etc.
    pub name: Rc<str>,
    /// The application's executable
    ///
    /// The absolute path to the application's executable, obtained from the `Exec` field in
    /// the desktop entry file
    pub exec: Option<Rc<str>>,
    /// The application's argv as declared in the desktop entry
    ///
    /// Tokens of the `Exec=` line starting from the primary binary (launcher prefixes such
    /// as `flatpak`/`snap`/`env` are stripped).
    ///
    /// Used to tell apart applications that share an `exec` path. For example Chromium PWAs
    /// (distinguished by `--app-id=`/`--app=`) or user-made launcher shortcuts for the
    /// same binary with different args.
    pub exec_args: Vec<Rc<str>>,
    /// The application's icon
    ///
    /// If available, taken verbatim from the `Icon` field in the desktop entry file
    pub icon: Option<Rc<str>>,
}

/// Returns a map of all applications available to the current user
///
/// Reads all desktop entry files in `$XDG_DATA_DIRS` and `$HOME/.local/share/applications`
/// and returns a map of the applications found.
///
/// The result is cached per-thread and invalidated lazily through inotify: each call
/// drains pending events on the watched `applications/` directories and only re-scans
/// when a `.desktop` file was created, deleted, modified, or moved. The first call
/// from a given thread does the initial scan and sets up the watches.
pub fn installed_apps() -> HashMap<Rc<str>, ApplicationEntry> {
    APPS_CACHE.with(|cell| {
        let mut slot = cell.borrow_mut();

        if let Some(cache) = slot.as_mut() {
            let dirty = drain_inotify_events(cache);
            ensure_watches(cache, env::xdg_data_dirs());
            if dirty {
                cache.apps = installed_apps_impl(env::xdg_data_dirs(), env::path());
            }
            return cache.apps.clone();
        }

        let apps = installed_apps_impl(env::xdg_data_dirs(), env::path());
        match Inotify::init(InitFlags::IN_NONBLOCK | InitFlags::IN_CLOEXEC) {
            Ok(inotify) => {
                let mut cache = AppsCache {
                    apps: apps.clone(),
                    inotify,
                    watched: HashMap::new(),
                };
                ensure_watches(&mut cache, env::xdg_data_dirs());
                *slot = Some(cache);
            }
            Err(e) => {
                log::warn!("inotify init failed ({e}); installed_apps() will not be cached");
            }
        }
        apps
    })
}

/// Returns a list of running applications
///
/// Requires a list of available applications and a list of running processes. The list of available
/// applications can be obtained using `installed_apps`.
///
/// Returns a list of running applications along with the PIDs for each running app.
///
/// **Example:**
/// ```
/// use app_rummage::{installed_apps, running_apps};
/// # struct MyProcess { pid: std::num::NonZeroU32, exe: Option<std::rc::Rc<str>> }
/// # impl app_rummage::Process for MyProcess { fn pid(&self) -> std::num::NonZeroU32 { self.pid } fn executable_path(&self) -> Option<std::path::PathBuf> { self.exe.as_ref().map(|e| std::path::Path::new(e.as_ref()).to_owned()) } fn name(&self) -> &str { "" } }
/// # fn running_processes() -> Vec<MyProcess> { vec![] }
///
/// let available_apps = installed_apps();
/// let processes = running_processes();
/// let running_apps = running_apps(&available_apps, &processes);
/// for (app, pids) in running_apps {
///    println!("{}: {:?}", app.name, pids);
/// }
/// ```
pub fn running_apps<'a, P: Process + 'a>(
    available_applications: &'a HashMap<Rc<str>, ApplicationEntry>,
    processes: impl IntoIterator<Item = &'a P>,
) -> Vec<(&'a ApplicationEntry, Vec<NonZeroU32>)> {
    let mut running_apps = running_xdg_conformant_apps(available_applications);

    let mut apps_by_proc = running_apps_by_process(available_applications, processes);
    for (app_id, (app, pids)) in apps_by_proc.drain() {
        let normalized_id = normalize_app_id(app_id.as_ref());
        if !running_apps.contains_key(normalized_id) {
            let key: Rc<str> = if normalized_id == app_id.as_ref() {
                app_id
            } else {
                Rc::from(normalized_id)
            };
            running_apps.insert(key, (app, pids));
        }
    }

    running_apps
        .values_mut()
        .map(|(app, pids)| (*app, std::mem::take(pids)))
        .collect()
}
