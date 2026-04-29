#![allow(dead_code)]
/* sys_info_v2/gatherer/src/platform/macos/services.rs
 *
 * Copyright 2024 Mission Center Contributors
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use crate::dbus_shim::{Append, Arg, ArgType, IterAppend, Signature};
use std::num::NonZeroU32;
use std::sync::Arc;

use crate::platform::services::{ServiceControllerExt, ServiceExt, ServicesExt};

#[derive(Debug, Clone, thiserror::Error)]
pub enum MacosServicesError {
    #[error("Services not supported on macOS")]
    NotSupported,
}

#[derive(Debug, Clone)]
pub struct MacosService {
    name: Arc<str>,
    description: Arc<str>,
    enabled: bool,
    running: bool,
    failed: bool,
    pid: Option<NonZeroU32>,
    user: Option<Arc<str>>,
    group: Option<Arc<str>>,
}

impl Default for MacosService {
    fn default() -> Self {
        Self {
            name: Arc::from(""),
            description: Arc::from(""),
            enabled: false,
            running: false,
            failed: false,
            pid: None,
            user: None,
            group: None,
        }
    }
}

impl ServiceExt for MacosService {
    fn name(&self) -> &str {
        self.name.as_ref()
    }
    fn description(&self) -> &str {
        self.description.as_ref()
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn running(&self) -> bool {
        self.running
    }
    fn failed(&self) -> bool {
        self.failed
    }
    fn pid(&self) -> Option<NonZeroU32> {
        self.pid
    }
    fn user(&self) -> Option<&str> {
        self.user.as_deref()
    }
    fn group(&self) -> Option<&str> {
        self.group.as_deref()
    }
}

pub struct MacosServiceController<'a> {
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a> ServiceControllerExt for MacosServiceController<'a> {
    type E = MacosServicesError;

    fn enable_service(&self, _name: &str) -> Result<(), Self::E> {
        Err(MacosServicesError::NotSupported)
    }
    fn disable_service(&self, _name: &str) -> Result<(), Self::E> {
        Err(MacosServicesError::NotSupported)
    }
    fn start_service(&self, _name: &str) -> Result<(), Self::E> {
        Err(MacosServicesError::NotSupported)
    }
    fn stop_service(&self, _name: &str) -> Result<(), Self::E> {
        Err(MacosServicesError::NotSupported)
    }
    fn restart_service(&self, _name: &str) -> Result<(), Self::E> {
        Err(MacosServicesError::NotSupported)
    }
}

pub struct MacosServices<'a> {
    _phantom: std::marker::PhantomData<&'a ()>,
}

impl<'a> Default for MacosServices<'a> {
    fn default() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<'a> MacosServices<'a> {
    pub fn new() -> Self { Self::default() }
}

impl<'a> ServicesExt<'a> for MacosServices<'a> {
    type S = MacosService;
    type C = MacosServiceController<'a>;
    type E = MacosServicesError;

    fn refresh_cache(&mut self) -> Result<(), Self::E> {
        Ok(())
    }

    fn services(&'a self) -> Result<Vec<Self::S>, Self::E> {
        Ok(vec![])
    }

    fn controller(&self) -> Result<Self::C, Self::E> {
        Ok(MacosServiceController {
            _phantom: std::marker::PhantomData,
        })
    }

    fn service_logs(&self, _name: &str, _pid: Option<NonZeroU32>) -> Result<Arc<str>, Self::E> {
        Ok(Arc::from(""))
    }
}

impl Arg for MacosService {
    const ARG_TYPE: ArgType = ArgType::Struct;
    fn signature() -> Signature { Signature::from("") }
}
impl Append for MacosService {
    fn append_by_ref(&self, _: &mut IterAppend) {}
}
