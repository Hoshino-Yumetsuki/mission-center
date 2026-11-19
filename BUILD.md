# Building Mission Center

Mission Center can be built natively for your host OS or packaged as an AppImage, Snap or Flatpak.

## Native

**Requirements:**

| Dependency                 | Comment             | Minimum Version |
|----------------------------|---------------------|----------------:|
| Meson                      |                     |           1.0.2 |
| Rust                       |                     |            1.96 |
| CMake                      |                     |            3.15 |
| Python3                    |                     |            3.10 |
| Blueprint Compiler         |                     |          0.19.0 |
| patch                      | Used to patch NVTOP |             N/A |
| DRM development libraries  |                     |             N/A |
| GBM development libraries  |                     |             N/A |
| udev development libraries |                     |             N/A |
| GLib                       |                     |            2.88 |
| GTK 4                      |                     |            4.22 |
| libadwaita                 |                     |             1.9 |

**Build instructions**

Note: A native build requires, at least, GLib 2.88, GTK 4.22 and libadwaita 1.9. That means ArchLinux >= 20260501,
Fedora >= 44, Ubuntu >= 26.04.

```bash
# The build needs the Git submodules, so clone with `--recursive`
git clone --recursive https://gitlab.com/mission-center-devs/mission-center
cd mission-center

# On Ubuntu 26.04 all dependencies, except for the Rust toolchain, can be installed with:
sudo apt install blueprint-compiler build-essential cmake curl desktop-file-utils gettext git libadwaita-1-dev libdrm-dev libgbm-dev libudev-dev meson pkg-config

BUILD_ROOT="$(pwd)/build-meson-debug"

meson setup "$BUILD_ROOT" -Dbuildtype=debug # Alternatively pass `-Dbuildtype=release` for a release build
ninja -C "$BUILD_ROOT"
```

### Running from the build directory

Running the app from the build directory (for development or debugging) requires some set up:

```bash
export PATH="$BUILD_ROOT/subprojects/magpie/src:$PATH"
export GSETTINGS_SCHEMA_DIR="$BUILD_ROOT/data"
export MC_MAGPIE_HW_DB="$BUILD_ROOT/subprojects/magpie/platform-linux/hwdb/hw.db"
export MC_RESOURCE_DIR="$BUILD_ROOT/resources"

glib-compile-schemas --strict --targetdir="$BUILD_ROOT/data" data
```

And then to run the app:

```bash
"$BUILD_ROOT/src/missioncenter"
```

### Installing

```bash
ninja -C "$BUILD_ROOT" install
```

And run the app from your launcher or from the command-line:

```bash
missioncenter
```

## AppImage

The AppImage can be built in two steps. Both steps are destructive since they install packages and replace system files,
so they should, ideally, be run in containers. The examples use Podman but Docker works just as well. The scripts are
meant to be run from the root of the source tree.

First, build Mission Center together with recent GTK and libadwaita releases in an Ubuntu 24.04 container. The result
is written to `_build/portable`:

```bash
podman run --rm -v "$(pwd)":/hostfs -e VERSION_STRING="$(git describe --tags --always)" \
    docker.io/library/ubuntu:24.04 bash /hostfs/support/build-with-gtk-libadwaita.sh
```

Then package it in an Arch Linux container:

```bash
podman run --rm -v "$(pwd)":/hostfs docker.io/library/archlinux:latest \
    bash -c 'cp -r /hostfs/_build/portable/usr / && bash /hostfs/support/create-appimage.sh'
```

The AppImage is written to the root of the source tree as `MissionCenter-<version>-<arch>.AppImage`. If building for
ARM64, use the `docker.io/pkgforge/archlinux:latest` image instead.

## Snap

The Snap package uses the output of the first AppImage step (`_build/portable`) and custom Snapcraft container, that
contains a fix that upstream won't merge.

After running the build step for the AppImage continue with:

```bash
mkdir -p snap/local
cp -r _build/portable/usr snap/local/
cp -r _build/portable/dependencies/* snap/local/

podman run --rm -v "$(pwd)":/project registry.gitlab.com/mission-center-devs/build-tools/snapcraft:x86-latest
```

The Snap is written to the root of the source tree as `mission-center_<version>_<arch>.snap`. If building for ARM64, use
the `arm-latest` tag.

## Flatpak

**Requirements:**

* Flatpak
* Flatpak-Builder

Add the `flathub` repo if not already present:

```bash
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
```

Install the required Flatpak runtimes and SDKs:

```bash
flatpak install -y \
    org.freedesktop.Platform//26.08 \
    org.freedesktop.Sdk//26.08 \
    org.gnome.Platform//51 \
    org.gnome.Sdk//51
```

Finally build a Flatpak package:

```bash
cd flatpak
flatpak-builder --repo=repo --ccache --force-clean build io.missioncenter.MissionCenter.json
flatpak build-bundle repo missioncenter.flatpak io.missioncenter.MissionCenter
```

Install the package:

```bash
flatpak uninstall -y io.missioncenter.MissionCenter
flatpak install -y missioncenter.flatpak
```

Run the app from your launcher or from the command-line:

```bash
flatpak run io.missioncenter.MissionCenter
```

## macOS

macOS support is experimental. Magpie (data collector) is vendored at `third_party/magpie` and symlinked as `subprojects/magpie`.

**Requirements (Homebrew):**

```bash
brew install gtk4 libadwaita glib librsvg blueprint-compiler pkg-config gettext protobuf
# optional for DMG packaging:
brew install create-dmg
```

See `scripts/macos/DEPENDENCIES.md` for details.

**Dev run (debug build + launch):**

```bash
git submodule update --init subprojects/graph-widget
./run-macos.sh
```

This will:

1. Build `magpie` for `aarch64-apple-darwin` from `third_party/magpie`
2. Generate `build-macos/src/config.rs` and build `missioncenter` with `BUILD_ROOT`
3. Compile Blueprint UI, GSettings schemas, and `missioncenter.gresource`
4. Stage `missioncenter-magpie` on `PATH` and launch the app

**Release package (`.app` + DMG):**

```bash
# Build release binaries first (or use .github/workflows/build-macos-dmg.yml)
(cd third_party/magpie && cargo build --release --target aarch64-apple-darwin)
mkdir -p build-macos && cp third_party/magpie/target/aarch64-apple-darwin/release/magpie build-macos/missioncenter-magpie
# compile resources/schemas/gresource + config.rs as in run-macos.sh / CI
BUILD_ROOT="$(pwd)/build-macos" cargo build --release --target aarch64-apple-darwin
bash scripts/macos/make-icns.sh
bash scripts/macos/package.sh
# -> dist/MissionCenter.dmg
```

Note: GTK/libadwaita are **not** bundled; they must be installed via Homebrew on the target machine.
