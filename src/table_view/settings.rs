/* table_view/settings.rs
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

use glib::g_critical;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gio, glib};

use crate::settings;
use crate::table_view::SettingsValues::*;
use crate::table_view::TableView;
use std::sync::atomic::{AtomicBool, Ordering};

static HIDE_ZEROES: AtomicBool = AtomicBool::new(false);
static TREE_VIEW: AtomicBool = AtomicBool::new(true);

pub fn configure(table_view: &TableView) {
    let settings = settings!();

    settings
        .bind(
            "apps-page-show-column-separators",
            &*table_view,
            "show-column-separators",
        )
        .build();

    table_view.set_use_merged_stats(merged_stats_enabled(&settings));
    let update_merged_stats = {
        let this = table_view.downgrade();
        move |settings: &gio::Settings, _: &str| {
            if let Some(this) = this.upgrade() {
                this.imp()
                    .use_merged_stats
                    .set(merged_stats_enabled(settings));
            }
        }
    };
    settings.connect_changed(
        Some("apps-page-merged-process-stats"),
        update_merged_stats.clone(),
    );
    settings.connect_changed(Some("apps-services-tree-view"), update_merged_stats);

    TREE_VIEW.store(
        settings.boolean("apps-services-tree-view"),
        Ordering::Relaxed,
    );

    settings.connect_changed(Some("apps-services-tree-view"), move |settings, _| {
        TREE_VIEW.store(
            settings.boolean("apps-services-tree-view"),
            Ordering::Relaxed,
        );
    });

    HIDE_ZEROES.store(
        settings.boolean("apps-page-hide-is-zero"),
        Ordering::Relaxed,
    );

    settings.connect_changed(Some("apps-page-hide-is-zero"), move |settings, _| {
        HIDE_ZEROES.store(
            settings.boolean("apps-page-hide-is-zero"),
            Ordering::Relaxed,
        );
    });

    configure_sorting(table_view, &settings);
}

pub fn is_hide_zeroes_active() -> bool {
    HIDE_ZEROES.load(Ordering::Relaxed)
}

pub fn is_tree_view() -> bool {
    TREE_VIEW.load(Ordering::Relaxed)
}

fn merged_stats_enabled(settings: &gio::Settings) -> bool {
    settings.boolean("apps-services-tree-view")
        && settings.boolean("apps-page-merged-process-stats")
}

fn configure_sorting(table_view: &TableView, settings: &gio::Settings) {
    let column_view = table_view.column_view();

    let sorting_column_name_key = &table_view.format_settings_key(&SortingColumnName);
    let sorting_order_key = &table_view.format_settings_key(&SortingOrder);

    if !settings.boolean("apps-page-remember-sorting") {
        let _ = settings.set_string(sorting_column_name_key, "");
        let _ = settings.set_enum(sorting_order_key, gtk::ffi::GTK_SORT_ASCENDING);
        return;
    }

    let saved_id = settings.string(sorting_column_name_key);
    let order = settings.enum_(sorting_order_key);

    let columns = column_view.columns();
    let mut matched_column = None;
    for i in 0..columns.n_items() {
        let Some(column) = columns
            .item(i)
            .and_then(|i| i.downcast::<gtk::ColumnViewColumn>().ok())
        else {
            continue;
        };
        let Some(column_id) = column.id() else {
            continue;
        };

        if column_id == saved_id.as_str() {
            matched_column = Some(column);
            break;
        }
    }

    let order = match order {
        gtk::ffi::GTK_SORT_ASCENDING => gtk::SortType::Ascending,
        gtk::ffi::GTK_SORT_DESCENDING => gtk::SortType::Descending,
        255 => return,
        _ => {
            g_critical!(
                "MissionCenter::ProcessTree",
                "Unknown column sorting order retrieved from settings, sorting in ascending order as a fallback"
            );
            gtk::SortType::Ascending
        }
    };
    column_view.sort_by_column(matched_column.as_ref(), order);
}
