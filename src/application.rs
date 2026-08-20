/* application.rs
 *
 * Copyright 2024 Romeo Calota
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

use std::cell::{BorrowError, Cell, Ref, RefCell};
use std::collections::HashMap;
use std::process::Command;

use adw::glib::g_warning;
use adw::{prelude::*, subclass::prelude::*};
use gtk::{
    gio,
    glib::{self, g_critical, property::PropertySet},
    Image,
};

use magpie_types::apps::icon::Icon;

use crate::about_system_dialog::AboutSystemDialog;
use crate::first_run_dialog::FirstRunDialog;
use crate::table_view::cached_icon::CachedIcon;
use crate::{config::VERSION, i18n::i18n, magpie_client::Readings};

pub const INTERVAL_STEP: f64 = 0.05;
pub const BASE_INTERVAL: f64 = 1f64;

#[macro_export]
macro_rules! app {
    () => {{
        use ::gtk::glib::object::Cast;
        ::gtk::gio::Application::default()
            .and_then(|app| app.downcast::<$crate::MissionCenterApplication>().ok())
            .expect("Failed to get MissionCenterApplication instance")
    }};
}

#[macro_export]
macro_rules! settings {
    () => {
        $crate::app!().settings()
    };
}

mod imp {
    use super::*;
    use crate::setup_readable_settings_cache;

    pub struct MissionCenterApplication {
        pub settings: gio::Settings,
        pub sys_info: RefCell<Option<crate::magpie_client::MagpieClient>>,
        pub window: RefCell<Option<crate::MissionCenterWindow>>,

        pub apps_icons_cache: Cell<Option<HashMap<String, CachedIcon>>>,
    }

    impl Default for MissionCenterApplication {
        fn default() -> Self {
            Self {
                settings: gio::Settings::new("io.missioncenter.MissionCenter"),
                sys_info: RefCell::new(None),
                window: RefCell::new(None),
                apps_icons_cache: Cell::new(None),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MissionCenterApplication {
        const NAME: &'static str = "MissioncenterApplication";
        type Type = super::MissionCenterApplication;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for MissionCenterApplication {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            obj.set_default();

            obj.setup_gactions();
            obj.set_accels_for_action("app.quit", &["<primary>q"]);
        }
    }

    impl ApplicationImpl for MissionCenterApplication {
        fn activate(&self) {
            use gtk::glib::*;

            let application = self.obj();
            // Get the current window or create one if necessary
            let window = if let Some(window) = application.window() {
                window
            } else {
                let settings = &self.settings;

                let sys_info = crate::magpie_client::MagpieClient::new();

                let window = crate::MissionCenterWindow::new(&*application, &settings, &sys_info);

                setup_readable_settings_cache(&settings);

                window.connect_default_height_notify({
                    move |window| {
                        let settings = settings!();
                        settings
                            .set_int("window-height", window.default_height())
                            .unwrap_or_else(|err| {
                                g_critical!(
                                    "MissionCenter",
                                    "Failed to save window height: {}",
                                    err
                                );
                            });
                    }
                });
                window.connect_default_width_notify({
                    move |window| {
                        let settings = settings!();
                        settings
                            .set_int("window-width", window.default_width())
                            .unwrap_or_else(|err| {
                                g_critical!(
                                    "MissionCenter",
                                    "Failed to save window width: {}",
                                    err
                                );
                            });
                    }
                });

                window
                    .set_default_size(settings.int("window-width"), settings.int("window-height"));

                window.connect_maximized_notify({
                    move |window| {
                        let settings = settings!();
                        settings
                            .set_boolean("is-maximized", window.is_maximized())
                            .unwrap_or_else(|err| {
                                g_critical!(
                                    "MissionCenter",
                                    "Failed to save window maximization: {}",
                                    err
                                );
                            });
                    }
                });

                window.set_maximized(settings.boolean("is-maximized"));

                sys_info.set_core_count_affects_percentages(
                    settings.boolean("apps-page-core-count-affects-percentages"),
                );

                settings.connect_changed(
                    Some("apps-page-core-count-affects-percentages"),
                    move |settings, _| {
                        let app = app!();
                        match app.sys_info() {
                            Ok(sys_info) => {
                                sys_info.set_core_count_affects_percentages(
                                    settings.boolean("apps-page-core-count-affects-percentages"),
                                );
                            }
                            Err(e) => {
                                g_critical!(
                                    "MissionCenter",
                                    "Failed to get sys_info from MissionCenterApplication: {}",
                                    e
                                );
                            }
                        };
                    },
                );

                self.sys_info.set(Some(sys_info));

                let provider = gtk::CssProvider::new();
                provider.load_from_bytes(&Bytes::from_static(include_bytes!(
                    "../resources/ui/style.css"
                )));

                gtk::style_context_add_provider_for_display(
                    &gtk::gdk::Display::default().expect("Could not connect to a display."),
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );

                #[cfg(target_os = "macos")]
                window.connect_close_request({
                    let application = application.downgrade();
                    move |_| {
                        if let Some(application) = application.upgrade() {
                            application.quit();
                        }
                        glib::Propagation::Stop
                    }
                });

                window.upcast()
            };

            window.present();

            self.window
                .set(window.downcast_ref::<crate::MissionCenterWindow>().cloned());
        }
    }

    impl GtkApplicationImpl for MissionCenterApplication {}

    impl AdwApplicationImpl for MissionCenterApplication {}
}

glib::wrapper! {
    pub struct MissionCenterApplication(ObjectSubclass<imp::MissionCenterApplication>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl MissionCenterApplication {
    pub fn apply_app_icon(&self, image: &Image, app_id: String, width: i32) -> bool {
        let this = self.imp();

        // if it was default::default, then this has no side effects and we can safely return
        let Some(mut icons) = this.apps_icons_cache.take() else {
            return false;
        };

        let retval = if let Some(mut icon) = icons.remove(&app_id) {
            icon.apply_to_image(image, width);

            icons.insert(app_id, icon);

            true
        } else {
            CachedIcon::apply_blank(image);

            false
        };

        this.apps_icons_cache.set(Some(icons));

        retval
    }

    pub fn new(application_id: &str, flags: &gio::ApplicationFlags) -> Self {
        use glib::g_message;

        let this: Self = glib::Object::builder()
            .property("application-id", application_id)
            .property("flags", flags)
            .build();

        g_message!(
            "MissionCenter::Application",
            "Starting Mission Center v{}",
            env!("CARGO_PKG_VERSION")
        );

        this
    }

    pub fn restart(&self) {
        #[cfg(unix)]
        use std::os::unix::process::CommandExt;

        let exe = std::env::current_exe().expect("Failed to get exe");

        #[cfg(unix)]
        {
            let _ = Command::new(exe).exec();
        }

        #[cfg(windows)]
        {
            Command::new(&exe)
                .spawn()
                .expect("Failed to spawn new process");

            std::process::exit(0);
        }
    }

    pub fn set_initial_readings(&self, readings: Readings) {
        use gtk::glib::*;

        let Some(window) = self.window() else {
            g_critical!(
                "MissionCenter::Application",
                "No active window, when trying to refresh data"
            );
            return;
        };

        window.set_initial_readings(readings)
    }

    pub fn set_app_icons(&self, icons: HashMap<String, Icon>) {
        self.imp()
            .apps_icons_cache
            .set(Some(CachedIcon::convert_hash_map(icons)))
    }

    pub fn merge_app_icons(&self, icons: HashMap<String, Icon>) {
        let old = self.imp().apps_icons_cache.take();

        let Some(mut old) = old else {
            self.set_app_icons(icons);
            return;
        };

        let icons = CachedIcon::convert_hash_map(icons);

        for (app_id, icon) in icons {
            old.insert(app_id, icon);
        }

        self.imp().apps_icons_cache.set(Some(old));
    }

    pub fn missing_icons(&self, mut appids: Vec<&String>) -> Option<Vec<String>> {
        let this = self.imp();
        let icons = this.apps_icons_cache.take();

        this.apps_icons_cache.set(icons.clone());

        let Some(apps) = icons else {
            return Some(appids.drain(..).map(|app_id| app_id.to_string()).collect());
        };

        let out: Vec<_> = appids
            .drain(..)
            .filter_map(|appid| {
                if !apps.contains_key(appid) {
                    Some(appid.clone())
                } else {
                    None
                }
            })
            .collect();

        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    pub fn setup_animations(&self) {
        use gtk::glib::*;

        let Some(window) = self.window() else {
            g_critical!(
                "MissionCenter::Application",
                "No active window, when trying to refresh data"
            );
            return;
        };

        window.setup_animations()
    }

    pub fn refresh_readings(&self, readings: &mut Readings) -> bool {
        use gtk::glib::*;

        let Some(window) = self.window() else {
            g_critical!(
                "MissionCenter::Application",
                "No active window, when trying to refresh data"
            );
            return false;
        };

        window.update_readings(readings)
    }

    pub fn settings(&self) -> gio::Settings {
        self.imp().settings.clone()
    }

    pub fn sys_info(&self) -> Result<Ref<'_, crate::magpie_client::MagpieClient>, BorrowError> {
        match self.imp().sys_info.try_borrow() {
            Ok(sys_info_ref) => Ok(Ref::map(sys_info_ref, |sys_info_opt| match sys_info_opt {
                Some(sys_info) => sys_info,
                None => {
                    panic!("MissionCenter::Application::sys_info() called before sys_info was initialized");
                }
            })),
            Err(e) => Err(e),
        }
    }

    pub fn window(&self) -> Option<crate::MissionCenterWindow> {
        self.imp().window.borrow().clone()
    }

    fn setup_gactions(&self) {
        let quit_action = gio::ActionEntry::builder("quit")
            .activate(move |app: &Self, _, _| app.quit())
            .build();
        let preferences_action = gio::ActionEntry::builder("preferences")
            .activate(move |app: &Self, _, _| {
                app.show_preferences();
            })
            .build();
        let about_action = gio::ActionEntry::builder("about")
            .activate(move |app: &Self, _, _| app.show_about())
            .build();
        let about_system_action = gio::ActionEntry::builder("system-about")
            .activate(move |app: &Self, _, _| app.show_system_about())
            .build();
        let keyboard_shortcuts_action = gio::ActionEntry::builder("keyboard-shortcuts")
            .activate(move |app: &Self, _, _| app.show_keyboard_shortcuts())
            .build();
        let open_wiki_action = gio::ActionEntry::builder("open-wiki")
            .activate(move |_, _, _| {
                gio::AppInfo::launch_default_for_uri(
                    "https://gitlab.com/mission-center-devs/mission-center/-/wikis/home",
                    None::<&gio::AppLaunchContext>,
                )
                .unwrap_or_else(|_| {
                    g_critical!(
                        "MissionCenter::Application",
                        "Failed to open Mission Center Wiki"
                    );
                });
            })
            .build();
        let first_run_dialog_action = gio::ActionEntry::builder("first-run-dialog")
            .activate(move |app: &Self, _, _| app.show_first_run_dialog())
            .build();

        self.add_action_entries([
            quit_action,
            preferences_action,
            about_action,
            about_system_action,
            keyboard_shortcuts_action,
            open_wiki_action,
            first_run_dialog_action,
        ]);

        self.set_accels_for_action("app.preferences", &["<Control>comma"]);
        self.set_accels_for_action("app.keyboard-shortcuts", &["<Control>question"]);
    }

    fn show_preferences(&self) {
        let Some(window) = self.window() else {
            g_critical!(
                "MissionCenter::Application",
                "No active window, when trying to show preferences"
            );
            return;
        };

        let preferences = crate::preferences::PreferencesDialog::new();
        preferences.present(Some(&window));
    }

    fn show_keyboard_shortcuts(&self) {
        let Some(app_window) = self.window() else {
            return;
        };

        let builder =
            gtk::Builder::from_resource("/io/missioncenter/MissionCenter/ui/keyboard_shortcuts.ui");
        let dialog = builder
            .object::<adw::ShortcutsDialog>("keyboard_shortcuts")
            .expect("Failed to get shortcuts window");

        dialog.present(Some(&app_window));
    }

    fn show_system_about(&self) {
        let app = app!();
        let Ok(magpie) = app.sys_info() else {
            g_warning!("MissionCenter::Disk", "Failed to get magpie client");
            return;
        };

        let about = magpie.about_system();

        let dialog = AboutSystemDialog::new(about);

        let Some(window) = self.window() else {
            g_critical!(
                "MissionCenter::Application",
                "No active window, when trying to show about dialog"
            );
            return;
        };

        dialog.present(Some(&window));
    }

    pub fn show_first_run_dialog(&self) {
        FirstRunDialog::run()
    }

    fn show_about(&self) {
        let Some(window) = self.window() else {
            g_critical!(
                "MissionCenter::Application",
                "No active window, when trying to show about dialog"
            );
            return;
        };

        let about = adw::AboutDialog::builder()
            .application_name("Mission Center")
            .application_icon("io.missioncenter.MissionCenter")
            .developer_name("Mission Center Developers")
            .developers(["Romeo Calota", "QwertyChouskie", "jojo2357", "Jan Luca"])
            .translator_credits(i18n("translator-credits"))
            .version(VERSION)
            .issue_url("https://gitlab.com/mission-center-devs/mission-center/-/issues")
            .copyright("© 2023-2025 Mission Center Developers")
            .license_type(gtk::License::Gpl30)
            .website("https://missioncenter.io")
            .release_notes(r#"<p>Noteworthy changes:</p>
<ul>
<li>Overhaul Services Page to include viewing child processes, user services, filtering on status, and a more efficient backend</li>
<li>Add an About System dialog that can be accessed from the context menu</li>
</ul>
<p>Minor features:</p>
<ul>
<li>Update to GNOME 49 Platform</li>
<li>Show CPU power Draw</li>
<li>Add ability to send various OS signals to processes</li>
</ul>
<p>Bug fixes:</p>
<ul>
<li>Ignore SMART temps of 0 Kelvin</li>
<li>Improve fans configuration</li>
<li>Reduce label formatter overhead</li>
<li>For GPUs, fix reading of max_link_{width,speed} and make reading current values more robust</li>
<li>Fix MemoryCompositionWidget tooltip offset</li>
<li>Reduce CPU usage when fetching and updating data</li>
</ul>
<p>Translation updates</p>
<ul>
<li>Arabic by jonnysemon</li>
<li>Basque by Ibai Oihanguren Sala</li>
<li>Belarusian by Yahor, teacond</li>
<li>Chinese (Simplified Han script) by flywater</li>
<li>Czech by Fjuro, erindesu, orangesunny, pavelbo</li>
<li>Dutch by philip.goto, Klinton_</li>
<li>Estonian by IndrekHaav</li>
<li>Finnish by artnay</li>
<li>French by Norbert V</li>
<li>Galician by Espasant3</li>
<li>German by Gian Veronese, Real Tehreal, dbstf</li>
<li>Hebrew by yarons</li>
<li>Hungarian by therealmate, KAMI911</li>
<li>Italian by FrecceNere, Kryotek, amivaleo</li>
<li>Irish by aindriu80</li>
<li>Japanese by shryt0206, rainy_sunset</li>
<li>Norwegian Bokmål by Telaneo</li>
<li>Polish by keloH, Real_Microwave, Cool guy</li>
<li>Portuguese by Raphael Campos, SantosSi</li>
<li>Portuguese (Brazil) by Raphael Campos, danick8989, flyrio</li>
<li>Russian by teacond</li>
<li>Spanish by nolddor, BrYellow, maxdesigna7x</li>
<li>Tamil by tace16</li>
<li>Turkish by yigitalcks, yakushabb</li>
<li>Ukrainian by Ethermidate</li>
</ul>"#)
            .build();

        about.add_credit_section(
            Some("Standing on the shoulders of giants"),
            &[
                "GTK https://www.gtk.org/",
                "GNOME https://www.gnome.org/",
                "Libadwaita https://gitlab.gnome.org/GNOME/libadwaita",
                "Blueprint Compiler https://jwestman.pages.gitlab.gnome.org/blueprint-compiler/",
                "NVTOP https://github.com/Syllo/nvtop",
                "Workbench https://github.com/sonnyp/Workbench",
                "And many more... Thank you all!",
            ],
        );

        about.present(Some(&window));
    }
}
