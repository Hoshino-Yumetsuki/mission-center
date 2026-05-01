# macOS External Dependencies

Install via Homebrew before running Mission Center:

```bash
brew install gtk4 libadwaita glib librsvg
```

| Package | Homebrew formula | Purpose |
|---|---|---|
| GTK 4 | `gtk4` | UI toolkit |
| libadwaita | `libadwaita` | GNOME HIG widgets |
| GLib / GIO | `glib` | Core GLib runtime, GSettings |
| librsvg | `librsvg` | SVG rendering (app icons) |

These are runtime dependencies — they are **not** bundled inside the DMG.
The app will fail to launch if they are missing.
