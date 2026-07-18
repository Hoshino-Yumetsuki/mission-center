use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::Path;
use std::rc::Rc;
use std::sync::OnceLock;

use crate::{env, ApplicationEntry, Process};

// Adapted from Resources (src/utils/app.rs)
fn executable_exceptions() -> &'static HashMap<&'static str, &'static str> {
    static EXECUTABLE_EXCEPTIONS: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();

    EXECUTABLE_EXCEPTIONS.get_or_init(|| {
        HashMap::from([
            ("firefox-bin", "firefox"),
            ("oosplash", "libreoffice"),
            ("soffice.bin", "libreoffice"),
            ("resources-processes", "resources"),
            ("gnome-terminal-server", "gnome-terminal"),
            ("chrome", "google-chrome-stable"),
        ])
    })
}

pub(crate) fn running_apps_by_process<'a, P: Process + 'a>(
    available_apps: &'a HashMap<Rc<str>, ApplicationEntry>,
    processes: impl IntoIterator<Item = &'a P>,
) -> HashMap<Rc<str>, (&'a ApplicationEntry, Vec<NonZeroU32>)> {
    let mut apps_by_exec: HashMap<&Path, Vec<&ApplicationEntry>> = HashMap::new();
    let mut apps_by_exec_basename: HashMap<&str, Vec<&ApplicationEntry>> = HashMap::new();

    for app in available_apps.values() {
        if let Some(exec) = app.exec.as_ref() {
            let exec = Path::new(exec.as_ref());
            apps_by_exec.entry(exec).or_default().push(app);

            if let Some(name) = exec.file_name().and_then(|n| n.to_str()) {
                apps_by_exec_basename.entry(name).or_default().push(app);
            }
        }
    }

    let mut result: HashMap<Rc<str>, (&ApplicationEntry, Vec<NonZeroU32>)> = HashMap::new();
    result.reserve(available_apps.len());

    for process in processes.into_iter() {
        let proc_exec = if let Some(exe) = process.executable_path() {
            exe
        } else {
            env::path()
                .iter()
                .find_map(|dir| {
                    let mut path = Path::new(dir).join(process.name());
                    if !path.exists() {
                        if let Some(alternate) = executable_exceptions().get(process.name()) {
                            path = Path::new(dir).join(alternate);
                        }
                    }
                    if path.exists() {
                        path.canonicalize().ok()
                    } else {
                        None
                    }
                })
                .unwrap_or_default()
        };

        if proc_exec.as_os_str().is_empty() {
            continue;
        }

        if let Some(bucket) = apps_by_exec.get(proc_exec.as_path()) {
            let matched = if bucket.len() == 1 {
                Some(bucket[0])
            } else {
                argv_disambiguate(bucket, process.cmdline())
            };
            if let Some(app) = matched {
                attribute(&mut result, app, process.pid());
            }
            continue;
        }

        // Direct exe-path miss. Try the basename-keyed `executable_exceptions` table:
        // some apps run under a binary name that differs from the one their desktop
        // entry advertises (e.g. `gnome-terminal-server` is owned by `gnome-terminal`).
        let Some(basename) = proc_exec.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(replacement) = executable_exceptions().get(basename) else {
            continue;
        };
        let Some(bucket) = apps_by_exec_basename.get(*replacement) else {
            continue;
        };
        let matched = if bucket.len() == 1 {
            Some(bucket[0])
        } else {
            argv_disambiguate(bucket, process.cmdline())
        };
        if let Some(app) = matched {
            attribute(&mut result, app, process.pid());
        }
    }

    result
}

/// Picks the unique app in `candidates` whose declared `Exec=` argv is most specifically
/// a subset of the running process's `cmdline`.
///
/// Matching rule: every token in `app.exec_args` (except skipping two-char field codes
/// like `%U`, `%F`, `%i`) must appear somewhere in `cmdline`. The candidate
/// with the highest number of required tokens wins. True ties (same required-token count,
/// both subsets) drop the process since apps cannot be told apart.
fn argv_disambiguate<'a>(
    candidates: &[&'a ApplicationEntry],
    cmdline: &[String],
) -> Option<&'a ApplicationEntry> {
    let mut best: Option<(&ApplicationEntry, usize)> = None;
    let mut tied = false;

    for app in candidates {
        let required: Vec<&str> = app
            .exec_args
            .iter()
            .skip(1)
            .map(|t| t.as_ref())
            .filter(|t| !is_field_code(t))
            .collect();

        if !required.iter().all(|t| cmdline.iter().any(|c| c == t)) {
            continue;
        }

        let score = required.len();
        match best {
            None => {
                best = Some((app, score));
                tied = false;
            }
            Some((_, current)) if score > current => {
                best = Some((app, score));
                tied = false;
            }
            Some((_, current)) if score == current => {
                tied = true;
            }
            Some(_) => {}
        }
    }

    if tied {
        None
    } else {
        best.map(|(app, _)| app)
    }
}

fn attribute<'a>(
    result: &mut HashMap<Rc<str>, (&'a ApplicationEntry, Vec<NonZeroU32>)>,
    app: &'a ApplicationEntry,
    pid: NonZeroU32,
) {
    match result.get_mut(&app.id) {
        Some((_, pids)) => {
            pids.push(pid);
            pids.sort_unstable();
        }
        None => {
            result.insert(app.id.clone(), (app, vec![pid]));
        }
    }
}

fn is_field_code(token: &str) -> bool {
    let bytes = token.as_bytes();
    bytes.len() == 2 && bytes[0] == b'%'
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_running_applications_process() {
        #[derive(Debug)]
        struct MyProcess {
            pid: NonZeroU32,
            name: Rc<str>,
            exe: Option<Rc<str>>,
        }

        impl Process for MyProcess {
            fn pid(&self) -> NonZeroU32 {
                self.pid
            }

            fn executable_path(&self) -> Option<PathBuf> {
                self.exe.as_ref().map(|e| Path::new(e.as_ref()).to_owned())
            }

            fn name(&self) -> &str {
                self.name.as_ref()
            }
        }

        fn running_processes() -> Vec<MyProcess> {
            let mut result = vec![];

            let readdir = match Path::new("/proc").read_dir() {
                Ok(r) => r,
                Err(_) => {
                    return vec![];
                }
            };

            for entry in readdir.filter_map(|e| e.ok()) {
                let path = entry.path();
                let mut exe = Rc::<str>::from("");
                if let Some(pid) = path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .and_then(|f| f.parse().ok())
                {
                    let bin_path = path.join("exe");
                    if let Ok(bin_path) =
                        std::fs::read_link(&bin_path).and_then(|p| p.canonicalize())
                    {
                        if bin_path.exists() {
                            exe = Rc::from(bin_path.to_string_lossy());
                        }
                    } else {
                        if let Some(bin_path) = std::fs::read_to_string(path.join("cmdline"))
                            .ok()
                            .and_then(|s| match s.split('\0').next() {
                                Some("") => None,
                                Some(s) => Some(s.to_owned()),
                                None => None,
                            })
                            .map(|s| Path::new(&s).to_owned())
                            .and_then(|p| p.canonicalize().ok())
                        {
                            if bin_path.exists() && bin_path.is_file() && bin_path.is_absolute() {
                                exe = Rc::from(bin_path.to_string_lossy());
                            }
                        }
                    }

                    let proc_name = path.join("comm");
                    if let Ok(name) = std::fs::read_to_string(&proc_name) {
                        result.push(MyProcess {
                            pid,
                            exe: Some(exe),
                            name: Rc::from(name.trim()),
                        });
                    }
                }
            }

            result
        }

        let available_apps = crate::installed_apps();
        let processes = running_processes();
        let result = running_apps_by_process(&available_apps, &processes);
        dbg!(&result);
    }

    #[test]
    fn running_apps_by_process_uses_executable_exceptions() {
        #[derive(Debug)]
        struct MyProcess {
            pid: NonZeroU32,
            name: Rc<str>,
            exe: PathBuf,
        }

        impl Process for MyProcess {
            fn pid(&self) -> NonZeroU32 {
                self.pid
            }
            fn executable_path(&self) -> Option<PathBuf> {
                Some(self.exe.clone())
            }
            fn name(&self) -> &str {
                self.name.as_ref()
            }
        }

        let mut available_apps: HashMap<Rc<str>, ApplicationEntry> = HashMap::new();
        let id: Rc<str> = Rc::from("org.gnome.Terminal");
        available_apps.insert(
            id.clone(),
            ApplicationEntry {
                id: id.clone(),
                name: Rc::from("Terminal"),
                exec: Some(Rc::from("/usr/bin/gnome-terminal")),
                exec_args: Vec::new(),
                icon: None,
            },
        );

        let processes = vec![MyProcess {
            pid: NonZeroU32::new(4242).unwrap(),
            name: Rc::from("gnome-terminal-"),
            exe: PathBuf::from("/usr/bin/gnome-terminal-server"),
        }];

        let result = running_apps_by_process(&available_apps, &processes);
        let (_, pids) = result
            .get("org.gnome.Terminal")
            .expect("gnome-terminal-server must be attributed via executable_exceptions");
        assert_eq!(pids, &vec![NonZeroU32::new(4242).unwrap()]);
    }
}
