/* src/gpus/gl_info.rs
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

use std::fmt::Write;
use std::os::fd::{AsFd, BorrowedFd};
use std::sync::OnceLock;

use arrayvec::ArrayString;
use gbm::AsRaw;
use nix::libc::c_void;

use magpie_platform::gpus::{ApiVersion, OpenGLVariant};

const EGL_CONTEXT_MAJOR_VERSION_KHR: egl::Int = 0x3098;
const EGL_CONTEXT_MINOR_VERSION_KHR: egl::Int = 0x30FB;
const EGL_PLATFORM_GBM_KHR: egl::Enum = 0x31D7;
const EGL_OPENGL_ES3_BIT: egl::Int = 0x0040;

struct DrmDevice(std::fs::File);

impl AsFd for DrmDevice {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl From<std::fs::File> for DrmDevice {
    fn from(file: std::fs::File) -> Self {
        Self(file)
    }
}

impl drm::Device for DrmDevice {}

fn egl() -> Option<&'static egl::DynamicInstance<egl::EGL1_2>> {
    static EGL: OnceLock<Option<egl::DynamicInstance<egl::EGL1_2>>> = OnceLock::new();
    EGL.get_or_init(|| {
        let lib_egl_so = match unsafe { libloading::Library::new("libEGL.so.1") } {
            Ok(lib) => lib,
            Err(e) => {
                log::error!("Failed to load libEGL.so.1: {e}");
                return None;
            }
        };

        match unsafe { egl::DynamicInstance::<egl::EGL1_2>::load_required_from(lib_egl_so) } {
            Ok(egl) => Some(egl),
            Err(e) => {
                log::error!("Failed to load EGL functions: {e}");
                None
            }
        }
    })
    .as_ref()
}

#[allow(non_snake_case)]
fn egl_display(
    dev: &gbm::Device<DrmDevice>,
    egl: &'static egl::DynamicInstance<egl::EGL1_2>,
) -> Option<egl::Display> {
    static EGL_GET_PLATFORM_DISPLAY_FN: OnceLock<
        Option<extern "C" fn(egl::Enum, *mut c_void, *const egl::Int) -> egl::EGLDisplay>,
    > = OnceLock::new();

    #[allow(clippy::missing_transmute_annotations)]
    let egl_get_platform_display = EGL_GET_PLATFORM_DISPLAY_FN.get_or_init(|| {
        let eglGetPlatformDisplayEXT = egl.get_proc_address("eglGetPlatformDisplayEXT");
        if let Some(eglGetPlatformDisplayEXT) = eglGetPlatformDisplayEXT {
            return Some(unsafe { std::mem::transmute(eglGetPlatformDisplayEXT) });
        };

        let eglGetPlatformDisplay = egl.get_proc_address("eglGetPlatformDisplay");
        if let Some(eglGetPlatformDisplay) = eglGetPlatformDisplay {
            return Some(unsafe { std::mem::transmute(eglGetPlatformDisplay) });
        };

        None
    });

    if let Some(egl_get_platform_display) = egl_get_platform_display {
        unsafe {
            Some(egl::Display::from_ptr(egl_get_platform_display(
                EGL_PLATFORM_GBM_KHR,
                dev.as_raw() as *mut c_void,
                std::ptr::null(),
            )))
        }
    } else {
        unsafe { egl.get_display(dev.as_raw() as *mut c_void) }
    }
}

pub unsafe fn supported_opengl_version(pci_id: &str) -> Option<(OpenGLVariant, ApiVersion)> {
    const DRI_PATH_LEN: usize = "/dev/dri/by-path/pci-".len() + 16 + "card".len();

    let egl = egl()?;

    let pci_id = match ArrayString::<16>::from(pci_id) {
        Ok(id) => id,
        Err(_) => {
            log::warn!("PCI ID exceeds 16 characters: {}", pci_id);
            return None;
        }
    };

    let mut dri_path = ArrayString::<DRI_PATH_LEN>::new();
    let _ = write!(&mut dri_path, "/dev/dri/by-path/pci-{pci_id}-card");

    let dri_file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dri_path)
        .or({
            let mut pci_id_lower = pci_id;
            for c in unsafe { pci_id_lower.as_bytes_mut() } {
                c.make_ascii_lowercase();
            }

            dri_path.clear();
            let _ = write!(&mut dri_path, "/dev/dri/by-path/pci-{pci_id_lower}-card");

            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(dri_path)
        })
        .or({
            let mut pci_id_upper = pci_id;
            for c in unsafe { pci_id_upper.as_bytes_mut() } {
                c.make_ascii_uppercase();
            }

            dri_path.clear();
            let _ = write!(&mut dri_path, "/dev/dri/by-path/pci-{pci_id_upper}-card");

            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(dri_path)
        }) {
        Ok(f) => f,
        Err(e) => {
            log::debug!(
                "Failed to get OpenGL information: Failed to open {}: {}",
                dri_path,
                e
            );
            return None;
        }
    };

    let gbm_device = match gbm::Device::new(DrmDevice::from(dri_file)) {
        Err(e) => {
            log::error!("Failed to get OpenGL information: {e}");
            return None;
        }
        Ok(gbm_device) => gbm_device,
    };

    let Some(egl_display) = egl_display(&gbm_device, egl) else {
        log::error!(
            "Failed to get OpenGL information: Failed to initialize an EGL display: {:?}",
            egl.get_error()
        );
        return None;
    };

    let (egl_major, egl_minor) = match egl.initialize(egl_display) {
        Ok(ver) => ver,
        Err(e) => {
            log::error!(
                "Failed to get OpenGL information: Failed to initialize an EGL display: {e:?}",
            );
            return None;
        }
    };

    if egl_major < 1 || (egl_major == 1 && egl_minor < 4) {
        log::error!(
            "Failed to get OpenGL information: EGL version 1.4 or higher is required to test OpenGL support"
        );
        return None;
    }

    let mut gl_api = egl::OPENGL_API;
    if egl.bind_api(gl_api).is_err() {
        gl_api = egl::OPENGL_ES_API;
        if let Err(error) = egl.bind_api(gl_api) {
            log::error!("Failed to get OpenGL information: Failed to bind an EGL API: {error:?}",);
            return None;
        }
    }

    let mut egl_configs = Vec::with_capacity(10);
    let error = if gl_api == egl::OPENGL_ES_API {
        let mut error = None;
        for es_bit in [EGL_OPENGL_ES3_BIT, egl::OPENGL_ES2_BIT, egl::OPENGL_ES_BIT] {
            let attrs = [
                egl::SURFACE_TYPE,
                egl::WINDOW_BIT,
                egl::RENDERABLE_TYPE,
                es_bit,
                egl::NONE,
            ];
            match egl.choose_config(egl_display, &attrs, &mut egl_configs) {
                Ok(()) => break,
                Err(e) => error = Some(e),
            }
        }
        error
    } else {
        let attrs = [
            egl::SURFACE_TYPE,
            egl::WINDOW_BIT,
            egl::RENDERABLE_TYPE,
            egl::OPENGL_BIT,
            egl::NONE,
        ];
        egl.choose_config(egl_display, &attrs, &mut egl_configs)
            .err()
    };

    let egl_config = match (egl_configs.drain(..).next(), error) {
        (Some(egl_config), _) => egl_config,
        (None, Some(error)) => {
            log::error!(
                "Failed to get OpenGL information: Failed to choose an EGL config: {error:?}",
            );
            return None;
        }
        (None, None) => {
            log::error!(
                "Failed to get OpenGL information: Failed to choose an EGL config: Unknown error",
            );
            return None;
        }
    };

    let mut ver_major = if gl_api == egl::OPENGL_API { 4 } else { 3 };
    let mut ver_minor = if gl_api == egl::OPENGL_API { 6 } else { 0 };

    let mut context_attribs = [
        EGL_CONTEXT_MAJOR_VERSION_KHR,
        ver_major,
        EGL_CONTEXT_MINOR_VERSION_KHR,
        ver_minor,
        egl::NONE,
    ];

    let mut egl_context;
    loop {
        egl_context = egl.create_context(egl_display, egl_config, None, &context_attribs);

        if egl_context.is_ok() || (ver_major == 1 && ver_minor == 0) {
            break;
        }

        if ver_minor > 0 {
            ver_minor -= 1;
        } else {
            ver_major -= 1;
            ver_minor = 9;
        }

        context_attribs[1] = ver_major;
        context_attribs[3] = ver_minor;
    }

    match egl_context {
        Ok(ec) => _ = egl.destroy_context(egl_display, ec),
        Err(e) => {
            log::error!(
                "Failed to get OpenGL information: Failed to create an EGL context: {e:?})",
            );
            return None;
        }
    };

    Some((
        if gl_api != egl::OPENGL_API {
            OpenGLVariant::OpenGles
        } else {
            OpenGLVariant::OpenGl
        },
        ApiVersion {
            major: ver_major as u32,
            minor: ver_minor as u32,
            patch: None,
        },
    ))
}
