# macOS External Dependencies

Install via Homebrew before building or running Mission Center:

```bash
brew install gtk4 libadwaita glib librsvg blueprint-compiler pkg-config gettext create-dmg
```

| Package | Homebrew formula | Purpose |
|---|---|---|
| GTK 4 | `gtk4` | UI toolkit |
| libadwaita | `libadwaita` | GNOME HIG widgets |
| GLib / GIO | `glib` | Core GLib runtime, GSettings |
| librsvg | `librsvg` | SVG rendering (app icons) |
| blueprint-compiler | `blueprint-compiler` | Compile `.blp` UI files |
| pkg-config | `pkg-config` | Find library flags at build time |
| gettext | `gettext` | i18n tooling |
| create-dmg | `create-dmg` | DMG packaging (CI / release only) |

Runtime GTK stack packages are **not** bundled inside the DMG.
The app will fail to launch if they are missing.

Magpie (the data collector) is built from `third_party/magpie` (symlinked as `subprojects/magpie`) and staged as `missioncenter-magpie` next to the app binary.
