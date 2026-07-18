/* src/gpus.rs
 *
 * Copyright 2025 Mission Center Developers
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

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use rayon::ThreadPool;

pub type ApiVersion = types::gpus::ApiVersion;
pub type OpenGLVariant = types::gpus::OpenGlVariant;
pub type ProcessKind = types::gpus::process::Kind;
pub type Process = types::gpus::Process;

pub type Gpu = types::gpus::Gpu;

pub type GpuMap = types::gpus::gpus_response::GpuMap;
pub type GpusResponseKind = types::gpus::gpus_response::Response;
pub type GpusRequest = types::gpus::GpusRequest;
pub type GpusResponse = types::gpus::GpusResponse;
pub type GpusResponseError = types::gpus::GpusResponseError;

pub trait GpuCache {
    fn new() -> Self
    where
        Self: Sized;

    fn refresh(&mut self);
    fn cached_entries(&self) -> &HashMap<String, Gpu>;
}

pub struct GpusHandler<GC>
where
    GC: GpuCache,
{
    pub(crate) gpus: Mutex<GC>,
    pub(crate) local_cache: Mutex<HashMap<String, Gpu>>,
    refreshing: AtomicBool,
}

impl<GC> Default for GpusHandler<GC>
where
    GC: GpuCache + Send,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<GC> GpusHandler<GC>
where
    GC: GpuCache + Send,
{
    pub fn new() -> Self {
        Self {
            gpus: Mutex::new(GC::new()),
            local_cache: Mutex::new(HashMap::new()),
            refreshing: AtomicBool::new(false),
        }
    }

    pub fn refresh(&self) {
        let mut gpus = self.gpus.lock();
        gpus.refresh();
        *self.local_cache.lock() = gpus.cached_entries().clone();
    }

    pub fn refresh_async(&'static self, thread_pool: &ThreadPool) {
        if self.refreshing.fetch_or(true, Ordering::AcqRel) {
            return;
        }

        thread_pool.spawn(move || {
            let start = std::time::Instant::now();

            self.refresh();
            self.refreshing.store(false, Ordering::Release);

            log::debug!("PERF: Refreshed GPU information in {:?}", start.elapsed());
        });
    }
}
