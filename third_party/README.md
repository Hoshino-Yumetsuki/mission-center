# Vendored upstream dependencies

| Path | Upstream | Pinned |
|------|----------|--------|
| `magpie/` | https://gitlab.com/mission-center-devs/gng | `d978cb4` (mission-center main submodule) |
| `magpie/platform-linux/crates/app-rummage/` | https://gitlab.com/mission-center-devs/app-detection | `92e5fe1` |

`subprojects/magpie` is a symlink to `third_party/magpie` so meson/flatpak keep working.

macOS support lives in `magpie/platform-macos/` (this fork).
