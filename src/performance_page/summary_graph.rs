/* performance_page/summary_graph.rs
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

use std::cell::Cell;
use std::collections::HashMap;

use adw::subclass::prelude::*;
use glib::{ParamSpec, Properties, Value};
use gtk::{gdk, glib, prelude::*};

use crate::performance_page::widgets::{GraphWidget, SidebarDropHint};
use crate::settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceType {
    Cpu,
    Memory,
    Disk,
    Network,
    Gpu,
    Fan,
    Battery,
    Unspecified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceOverride {
    Show,
    Hide,
}

pub fn parse_device_overrides(raw: &str) -> HashMap<String, DeviceOverride> {
    raw.split(';')
        .filter(|s| !s.is_empty())
        .filter_map(|entry| {
            let (name, action) = entry.rsplit_once(':')?;
            let ovr = match action {
                "show" => DeviceOverride::Show,
                "hide" => DeviceOverride::Hide,
                _ => return None,
            };
            Some((name.to_string(), ovr))
        })
        .collect()
}

pub fn serialize_device_overrides(overrides: &HashMap<String, DeviceOverride>) -> String {
    overrides
        .iter()
        .map(|(name, ovr)| {
            let action = match ovr {
                DeviceOverride::Show => "show",
                DeviceOverride::Hide => "hide",
            };
            format!("{}:{}", name, action)
        })
        .collect::<Vec<_>>()
        .join(";")
}

pub fn resolve_device_visibility(
    device_name: &str,
    overrides: &HashMap<String, DeviceOverride>,
    category_visible: bool,
) -> bool {
    match overrides.get(device_name) {
        Some(DeviceOverride::Show) => true,
        Some(DeviceOverride::Hide) => false,
        None => category_visible,
    }
}

mod imp {
    use super::*;
    use crate::performance_page::widgets::GraphWidget;
    use std::marker::PhantomData;

    #[derive(Properties)]
    #[properties(wrapper_type = super::SummaryGraph)]
    #[derive(gtk::CompositeTemplate)]
    #[template(resource = "/io/missioncenter/MissionCenter/ui/performance_page/summary_graph.ui")]
    #[allow(dead_code)]
    pub struct SummaryGraph {
        pub drop_hint: SidebarDropHint,

        #[template_child]
        pub drag_handle_icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub graph_widget: TemplateChild<GraphWidget>,
        #[template_child]
        label_heading: TemplateChild<gtk::Label>,
        #[template_child]
        label_info1: TemplateChild<gtk::Label>,
        #[template_child]
        label_info2: TemplateChild<gtk::Label>,
        #[template_child]
        pub enabled_switch: TemplateChild<gtk::Switch>,

        #[property(get = Self::is_enabled, set = Self::set_enabled)]
        is_enabled: PhantomData<bool>,

        #[property(get = Self::base_color, set = Self::set_base_color)]
        base_color: PhantomData<gdk::RGBA>,
        #[property(get = Self::heading, set = Self::set_heading)]
        heading: PhantomData<String>,
        #[property(get = Self::info1, set = Self::set_info1)]
        info1: PhantomData<String>,
        #[property(get = Self::info2, set = Self::set_info2)]
        info2: PhantomData<String>,

        pub device_type: Cell<DeviceType>,

        pub user_activated: Cell<bool>,
    }

    impl Default for SummaryGraph {
        fn default() -> Self {
            Self {
                drop_hint: SidebarDropHint::new(),
                drag_handle_icon: Default::default(),
                graph_widget: Default::default(),
                label_heading: Default::default(),
                label_info1: Default::default(),
                label_info2: Default::default(),
                enabled_switch: Default::default(),

                is_enabled: PhantomData,

                base_color: PhantomData,
                heading: PhantomData,
                info1: PhantomData,
                info2: PhantomData,

                device_type: Cell::new(DeviceType::Unspecified),

                user_activated: Cell::new(true),
            }
        }
    }

    impl SummaryGraph {
        fn is_enabled(&self) -> bool {
            self.enabled_switch.is_active()
        }

        fn set_enabled(&self, enabled: bool) {
            self.enabled_switch.set_active(enabled);
        }

        fn base_color(&self) -> gdk::RGBA {
            self.graph_widget.base_color()
        }

        fn set_base_color(&self, base_color: gdk::RGBA) {
            self.graph_widget.set_base_color(base_color);
        }

        fn heading(&self) -> String {
            self.label_heading.text().to_string()
        }

        fn set_heading(&self, heading: String) {
            self.label_heading.set_text(&heading);
        }

        fn info1(&self) -> String {
            self.label_info1.text().to_string()
        }

        fn set_info1(&self, info1: String) {
            self.label_info1.set_text(&info1);
            if info1.is_empty() {
                self.label_info1.set_visible(false);
            } else {
                self.label_info1.set_visible(true);
            }
        }

        fn info2(&self) -> String {
            self.label_info2.text().to_string()
        }

        fn set_info2(&self, info2: String) {
            self.label_info2.set_text(&info2);
            if info2.is_empty() {
                self.label_info2.set_visible(false);
            } else {
                self.label_info2.set_visible(true);
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SummaryGraph {
        const NAME: &'static str = "SummaryGraph";
        type Type = super::SummaryGraph;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SummaryGraph {
        fn properties() -> &'static [ParamSpec] {
            Self::derived_properties()
        }

        fn set_property(&self, id: usize, value: &Value, pspec: &ParamSpec) {
            self.derived_set_property(id, value, pspec);
        }

        fn property(&self, id: usize, pspec: &ParamSpec) -> Value {
            self.derived_property(id, pspec)
        }

        fn constructed(&self) {
            self.parent_constructed();

            self.enabled_switch.connect_active_notify({
                let this = self.obj().downgrade();
                move |switch| {
                    let this = match this.upgrade() {
                        Some(this) => this,
                        None => return,
                    };

                    if !this.imp().user_activated.get() {
                        return;
                    }

                    let settings = settings!();

                    let raw = settings.string("performance-sidebar-device-overrides");
                    let mut overrides = parse_device_overrides(&raw);
                    let name = this.widget_name();

                    if switch.is_active() {
                        overrides.insert(name.to_string(), DeviceOverride::Show);
                    } else {
                        overrides.insert(name.to_string(), DeviceOverride::Hide);
                    }

                    let output = serialize_device_overrides(&overrides);

                    settings
                        .set_string("performance-sidebar-device-overrides", &output)
                        .unwrap_or_else(|_| {
                            glib::g_warning!(
                                "MissionCenter::PerformancePage",
                                "Failed to set performance-sidebar-device-overrides setting"
                            );
                        });

                    this.notify_is_enabled();
                }
            });
        }
    }

    impl WidgetImpl for SummaryGraph {}

    impl BoxImpl for SummaryGraph {}
}

glib::wrapper! {
    pub struct SummaryGraph(ObjectSubclass<imp::SummaryGraph>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::ConstraintTarget, gtk::Accessible, gtk::Buildable;
}

impl SummaryGraph {
    pub fn new(device_type: DeviceType) -> Self {
        let this: SummaryGraph = glib::Object::builder().build();
        this.imp().device_type.set(device_type);
        this
    }

    pub fn set_edit_mode(&self, edit_mode: bool, switch_enabled: bool) {
        let imp = self.imp();
        imp.drag_handle_icon.set_visible(edit_mode);
        imp.enabled_switch.set_visible(edit_mode);

        if edit_mode {
            self.set_switch_active(switch_enabled);
        }

        if let Some(parent) = self.parent() {
            parent.set_visible(edit_mode || switch_enabled);
        }
    }

    pub fn set_switch_active(&self, active: bool) {
        let imp = self.imp();
        imp.user_activated.set(false);
        imp.enabled_switch.set_active(active);
        imp.user_activated.set(true);
    }

    pub fn graph_widget(&self) -> GraphWidget {
        self.imp().graph_widget.clone()
    }

    pub fn show_drop_hint_top(&self) {
        self.hide_drop_hint();

        self.imp().drop_hint.set_margin_bottom(20);

        self.prepend(&self.imp().drop_hint);
    }

    pub fn show_drop_hint_bottom(&self) {
        self.hide_drop_hint();

        self.imp().drop_hint.set_margin_top(20);

        self.append(&self.imp().drop_hint);
    }

    pub fn hide_drop_hint(&self) {
        if !self.is_drop_hint_visible() {
            return;
        }

        self.imp().drop_hint.set_margin_top(0);
        self.imp().drop_hint.set_margin_bottom(0);

        self.remove(&self.imp().drop_hint);
    }

    pub fn is_drop_hint_visible(&self) -> bool {
        self.imp().drop_hint.parent().is_some()
    }

    pub fn is_drop_hint_top(&self) -> bool {
        if !self.is_drop_hint_visible() {
            return false;
        }

        self.imp().drop_hint.prev_sibling().is_none()
    }

    pub fn is_drop_hint_bottom(&self) -> bool {
        if !self.is_drop_hint_visible() {
            return false;
        }

        self.imp().drop_hint.next_sibling().is_none()
    }

    pub fn device_type(&self) -> DeviceType {
        self.imp().device_type.get()
    }
}
