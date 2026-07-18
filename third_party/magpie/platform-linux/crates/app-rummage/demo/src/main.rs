use std::collections::HashMap;
use std::env;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use eframe::egui;

use app_rummage::{ApplicationEntry, Process};

#[derive(Debug)]
struct MyProcess {
    pid: NonZeroU32,
    exe: Option<Rc<str>>,
    name: Rc<str>,
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
        Err(e) => {
            eprintln!("Error reading /proc: {}", e);
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
            if let Ok(bin_path) = std::fs::read_link(&bin_path).and_then(|p| p.canonicalize()) {
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

#[derive(Default)]
struct AppDemo {
    installed_apps: HashMap<Rc<str>, ApplicationEntry>,
    icon_theme: String,
    icon_dirs: Vec<String>,
}

impl AppDemo {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let icon_theme = dconf::read_string("/org/gnome/desktop/interface/icon-theme")
            .unwrap_or(Some("hicolor".to_string()))
            .unwrap_or("hicolor".to_string());

        let installed_apps = app_rummage::installed_apps();

        Self {
            installed_apps,
            icon_theme,
            icon_dirs: vec![
                "/usr/share/icons".to_owned(),
                "/usr/local/share/icons".to_owned(),
                "/var/lib/flatpak/exports/share/icons".to_owned(),
                "/var/lib/snapd/desktop".to_owned(),
                format!(
                    "{}/.local/share",
                    env::var("HOME").unwrap_or_else(|_| "/home".into())
                ),
                format!(
                    "{}/.icons",
                    env::var("HOME").unwrap_or_else(|_| "/home".into())
                ),
            ],
        }
    }
}

impl eframe::App for AppDemo {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let processes = running_processes();
        let mut running_apps = app_rummage::running_apps(&self.installed_apps, &processes);
        running_apps.sort_unstable_by(|(app1, _), (app2, _)| (*app1).name.cmp(&(*app2).name));
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Running apps");

            ui.vertical(|ui| {
                for (app, pids) in &running_apps {
                    ui.horizontal(|ui| {
                        let icon = app
                            .icon
                            .as_ref()
                            .map(|i| i.as_ref())
                            .unwrap_or("application-x-executable");

                        let mut icon_path = if Path::new(icon).is_absolute() {
                            icon.to_owned()
                        } else {
                            let mut icon_path = String::new();

                            'search: for dir in &self.icon_dirs {
                                for dir in [
                                    format!("{}/{}/scalable/apps", dir, self.icon_theme),
                                    format!("{}/{}/apps/scalable", dir, self.icon_theme),
                                    format!("{}/{}/512x512/apps", dir, self.icon_theme),
                                    format!("{}/{}/apps/512x512", dir, self.icon_theme),
                                    format!("{}/{}/256x256/apps", dir, self.icon_theme),
                                    format!("{}/{}/apps/256x256", dir, self.icon_theme),
                                    format!("{}/{}/128x128/apps", dir, self.icon_theme),
                                    format!("{}/{}/apps/128x128", dir, self.icon_theme),
                                    format!("{}/{}/64x64/apps", dir, self.icon_theme),
                                    format!("{}/{}/apps/64x64", dir, self.icon_theme),
                                    format!("{}/{}/32x32/apps", dir, self.icon_theme),
                                    format!("{}/{}/apps/32x32", dir, self.icon_theme),
                                    format!("{}/hicolor/scalable/apps", dir),
                                ] {
                                    for icon in [
                                        format!("{}/{}.svg", dir, icon),
                                        format!("{}/{}.svgz", dir, icon),
                                        format!("{}/{}.png", dir, icon),
                                    ] {
                                        if Path::new(&icon).exists() {
                                            icon_path = icon;
                                            break 'search;
                                        }
                                    }
                                }
                            }

                            icon_path
                        };
                        icon_path.insert_str(0, "file://");
                        ui.image(icon_path);

                        ui.label(app.name.as_ref());
                        ui.label(format!("({:?})", app.exec.as_ref()));
                        ui.label(format!("{:?}", pids.first()));
                    });
                }
            });

            let ctx = ctx.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(1));
                ctx.request_repaint();
            })
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "App detection demo",
        options,
        Box::new(|cc| Ok(Box::new(AppDemo::new(cc)))),
    )
}
