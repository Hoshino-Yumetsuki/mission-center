#![allow(dead_code)]
/* sys_info_v2/gatherer/src/platform/macos/apps.rs
 *
 * Copyright 2024 Mission Center Contributors
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use crate::dbus_shim::{Append, Arg, ArgType, IterAppend, Signature};
use std::sync::Arc;

use crate::platform::apps::{AppExt, AppsExt};

#[derive(Debug, Clone)]
pub struct MacosApp {
    name: Arc<str>,
    icon: Option<Arc<str>>,
    id: Arc<str>,
    command: Arc<str>,
    pids: Vec<u32>,
}

impl Default for MacosApp {
    fn default() -> Self {
        Self {
            name: Arc::from(""),
            icon: None,
            id: Arc::from(""),
            command: Arc::from(""),
            pids: vec![],
        }
    }
}

impl<'a> AppExt<'a> for MacosApp {
    type Iter = std::slice::Iter<'a, u32>;

    fn name(&self) -> &str {
        self.name.as_ref()
    }
    fn icon(&self) -> Option<&str> {
        self.icon.as_deref()
    }
    fn id(&self) -> &str {
        self.id.as_ref()
    }
    fn command(&self) -> &str {
        self.command.as_ref()
    }
    fn pids(&'a self) -> Self::Iter {
        self.pids.iter()
    }
}

#[derive(Default)]
pub struct MacosApps {
    apps: Vec<MacosApp>,
}

impl MacosApps {
    pub fn new() -> Self { Self::default() }
}

impl<'a> AppsExt<'a> for MacosApps {
    type A = MacosApp;
    type P = super::processes::MacosProcess;

    fn refresh_cache(&mut self, _processes: &std::collections::HashMap<u32, Self::P>) {
        self.apps.clear();
    }

    fn app_list(&self) -> &[Self::A] {
        &self.apps
    }
}

impl Arg for MacosApp {
    const ARG_TYPE: ArgType = ArgType::Struct;
    fn signature() -> Signature { Signature::from("") }
}
impl Append for MacosApp {
    fn append_by_ref(&self, _: &mut IterAppend) {}
}

impl Arg for MacosApps {
    const ARG_TYPE: ArgType = ArgType::Struct;
    fn signature() -> Signature { Signature::from("") }
}
impl Append for MacosApps {
    fn append_by_ref(&self, _: &mut IterAppend) {}
}
