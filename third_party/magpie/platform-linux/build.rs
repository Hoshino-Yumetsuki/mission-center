/* build.rs
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
use std::io::Read;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

fn find_program(name: &str) -> Option<String> {
    let path = if let Ok(path) = std::env::var("PATH") {
        path
    } else {
        "".into()
    };
    for path in path.split(':') {
        let program_path = format!("{}/{}", path, name);
        if std::path::Path::new(&program_path).exists() {
            return Some(program_path);
        }
    }

    None
}

fn download_file(
    url: &str,
    path: &str,
    checksum: Option<&str>,
) -> Result<String, Box<dyn std::error::Error>> {
    use std::io::Write;

    std::fs::create_dir_all(path)?;

    let file_name = url.split('/').next_back().unwrap();
    let output_path = format!("{}/{}", path, file_name);

    if let Ok(content) = std::fs::read(&output_path) {
        let mut sha256 = cargo_util::Sha256::new();
        sha256.update(&content);
        if checksum.is_none_or(|c| sha256.finish_hex() == c) {
            return Ok(output_path);
        }
    }

    let mut response = ureq::get(url).call()?;

    if response.status() != 200 {
        return Err(format!(
            "Failed to download {}. HTTP status code {}",
            url,
            response.status(),
        )
        .into());
    }

    let mut content = Vec::new();
    response.body_mut().as_reader().read_to_end(&mut content)?;

    let mut sha256 = cargo_util::Sha256::new();
    sha256.update(&content);

    if let Some(expected_checksum) = checksum {
        let actual_checksum = sha256.finish_hex();
        if actual_checksum != expected_checksum {
            return Err(format!(
                "Checksum validation failed! Expected: {} Actual: {}",
                expected_checksum, actual_checksum,
            )
            .into());
        }
    }

    let mut file = std::fs::File::create(&output_path)?;
    file.write_all(content.as_slice())?;
    file.flush()?;

    Ok(output_path)
}

fn patch_program() -> &'static str {
    static PATCH_EXECUTABLE: OnceLock<String> = OnceLock::new();

    PATCH_EXECUTABLE
        .get_or_init(|| match std::env::var("MC_PATCH_BINARY") {
            Ok(p) => {
                if std::path::Path::new(&p).exists() {
                    p
                } else {
                    eprintln!("{} does not exist", p);
                    std::process::exit(1);
                }
            }
            Err(_) => find_program("patch").unwrap_or_else(|| {
                eprintln!("`patch` not found");
                std::process::exit(1);
            }),
        })
        .as_str()
}

const PATCHES_STAMP: &str = ".magpie-patches-sha256";

fn patches_fingerprint(
    package_dir: &std::path::Path,
    manifest: &str,
    patches: &[String],
) -> Result<String, Box<dyn std::error::Error>> {
    let mut sha256 = cargo_util::Sha256::new();
    sha256.update(manifest.as_bytes());
    for patch in patches {
        let content = std::fs::read(package_dir.join(patch))?;
        sha256.update(&(content.len() as u64).to_le_bytes());
        sha256.update(&content);
    }

    Ok(sha256.finish_hex())
}

fn prepare_third_party_sources(
) -> Result<HashMap<String, std::path::PathBuf>, Box<dyn std::error::Error>> {
    let third_party_path = std::path::PathBuf::from(&format!(
        "{}/3rdparty",
        std::env::var("CARGO_MANIFEST_DIR")?
    ));
    println!("cargo:rerun-if-changed={}", third_party_path.display());

    let mut out_dir = std::env::var("OUT_DIR")?;
    out_dir.push_str("/../../native");
    std::fs::create_dir_all(&out_dir)?;
    let out_dir = std::path::PathBuf::from(out_dir).canonicalize()?;

    let mut result = HashMap::new();

    for dir in std::fs::read_dir(&third_party_path)?.filter_map(|d| d.ok()) {
        if !dir.file_type()?.is_dir() {
            continue;
        }

        for entry in std::fs::read_dir(dir.path())?.filter_map(|e| e.ok()) {
            let file_name = entry.file_name();
            let entry_name = file_name.to_string_lossy();
            if entry_name.ends_with(".json") {
                let manifest = std::fs::read_to_string(entry.path())?;
                let mut package: Package = serde_json::from_str(&manifest)?;

                let extracted_path = out_dir.join(&package.directory);
                result.insert(std::mem::take(&mut package.name), extracted_path.clone());

                let fingerprint = patches_fingerprint(&dir.path(), &manifest, &package.patches)?;
                if extracted_path.exists() {
                    match std::fs::read_to_string(extracted_path.join(PATCHES_STAMP)) {
                        Ok(stamp) if stamp == fingerprint => break,
                        Ok(_) => {}
                        // Sources prepared outside of this script, e.g. by the Flatpak manifest
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                        Err(e) => return Err(e.into()),
                    }
                }

                let output_path = download_file(
                    &package.source_url,
                    &format!("{}", out_dir.display()),
                    Some(&package.source_hash),
                )?;

                let staging_dir = out_dir.join(format!("{}.staging", package.directory));
                if staging_dir.exists() {
                    std::fs::remove_dir_all(&staging_dir)?;
                }
                std::fs::create_dir_all(&staging_dir)?;

                let mut archive = std::fs::File::open(&output_path)?;
                let tar = flate2::read::GzDecoder::new(&mut archive);
                let mut archive = tar::Archive::new(tar);
                archive.unpack(&staging_dir)?;

                let staged_path = staging_dir.join(&package.directory);
                for patch in package.patches.iter().map(|p| p.as_str()) {
                    let mut cmd = std::process::Command::new(patch_program());
                    cmd.args(["-p1", "-i", &format!("{}/{}", dir.path().display(), patch)]);
                    cmd.current_dir(&staged_path);
                    cmd.stdout(std::process::Stdio::inherit())
                        .stderr(std::process::Stdio::inherit());
                    let status = cmd.status()?;
                    if !status.success() {
                        let _ = std::fs::remove_dir_all(&staging_dir);
                        return Err(format!(
                            "Failed to apply {} to {}: {}",
                            patch, package.directory, status
                        )
                        .into());
                    }
                }

                std::fs::write(staged_path.join(PATCHES_STAMP), &fingerprint)?;
                if extracted_path.exists() {
                    std::fs::rename(&extracted_path, staging_dir.join("stale"))?;
                }
                std::fs::rename(&staged_path, &extracted_path)?;
                let _ = std::fs::remove_dir_all(&staging_dir);

                break;
            }
        }
    }

    Ok(result)
}

#[derive(Serialize, Deserialize)]
struct Package {
    #[serde(rename = "package-name")]
    name: String,
    directory: String,
    #[serde(rename = "source-url")]
    source_url: String,
    #[serde(rename = "source-hash")]
    source_hash: String,
    patches: Vec<String>,
}

fn build_nvtop(src_dir: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let _ = pkg_config::Config::new().probe("gbm")?;

    let libdrm = pkg_config::Config::new()
        .atleast_version("2.4.67")
        .probe("libdrm")?;

    // Work around some linkers being stupid and requiring a certain order for libraries
    libdrm.link_paths.iter().for_each(|p| {
        println!("cargo:rustc-link-search=native={}", p.display());
    });
    libdrm.libs.iter().for_each(|a| {
        println!("cargo:rustc-link-arg=-l{}", a);
    });

    let libudev = pkg_config::Config::new()
        .atleast_version("204")
        .probe("libudev")?;

    // Work around some linkers being stupid and requiring a certain order for libraries
    libudev.link_paths.iter().for_each(|p| {
        println!("cargo:rustc-link-search=native={}", p.display());
    });
    libudev.libs.iter().for_each(|a| {
        println!("cargo:rustc-link-arg=-l{}", a);
    });

    let mut build_def = cc::Build::new();
    build_def
        .define("USING_LIBUDEV", None)
        .define("_GNU_SOURCE", None)
        .include(src_dir.join("src"))
        .include(src_dir.join("include"))
        .includes(&libdrm.include_paths)
        .includes(&libudev.include_paths)
        .files([
            src_dir.join("src/get_process_info_linux.c"),
            src_dir.join("src/extract_gpuinfo.c"),
            src_dir.join("src/extract_processinfo_fdinfo.c"),
            src_dir.join("src/info_messages_linux.c"),
            src_dir.join("src/device_discovery_linux.c"),
            src_dir.join("src/extract_gpuinfo_ascend.c"),
            src_dir.join("src/extract_gpuinfo_ixml.c"),
            src_dir.join("src/extract_gpuinfo_metax.c"),
            src_dir.join("src/extract_gpuinfo_tenstorrent.c"),
            src_dir.join("src/extract_gpuinfo_tpu.c"),
            src_dir.join("src/extract_npuinfo_rockchip.c"),
            src_dir.join("src/extract_gpuinfo_amdgpu.c"),
            src_dir.join("src/extract_gpuinfo_amdgpu_utils.c"),
            src_dir.join("src/extract_gpuinfo_intel.c"),
            src_dir.join("src/extract_gpuinfo_intel_i915.c"),
            src_dir.join("src/extract_gpuinfo_intel_xe.c"),
            src_dir.join("src/extract_gpuinfo_mali_common.c"),
            src_dir.join("src/extract_gpuinfo_msm.c"),
            src_dir.join("src/extract_gpuinfo_msm_utils.c"),
            src_dir.join("src/extract_gpuinfo_nvidia.c"),
            src_dir.join("src/extract_gpuinfo_panfrost.c"),
            src_dir.join("src/extract_gpuinfo_panfrost_utils.c"),
            src_dir.join("src/extract_gpuinfo_panthor.c"),
            src_dir.join("src/extract_gpuinfo_panthor_utils.c"),
            src_dir.join("src/extract_gpuinfo_radeon.c"),
            src_dir.join("src/extract_gpuinfo_v3d.c"),
            src_dir.join("src/extract_gpuinfo_v3d_utils.c"),
            src_dir.join("src/time.c"),
        ]);
    build_def.flag("-Wno-unused-function");
    build_def.flag("-Wno-unused-variable");
    build_def.flag("-Wno-return-type");
    build_def.flag("-Wno-pointer-sign");

    build_def.flag_if_supported("-fno-reorder-blocks-and-partition");

    build_def.compile("nvtop");

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dirs = prepare_third_party_sources()?;
    build_nvtop(
        dirs.get("nvtop")
            .expect("NVTOP sources were not properly prepared"),
    )?;

    Ok(())
}
