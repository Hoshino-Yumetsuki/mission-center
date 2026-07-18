#!/bin/bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BUILD_DIR="$REPO_ROOT/build-macos"
DIST_DIR="$REPO_ROOT/dist"
APP_NAME="MissionCenter"
APP_BUNDLE="$DIST_DIR/$APP_NAME.app"
TARGET="${CARGO_BUILD_TARGET:-aarch64-apple-darwin}"
VERSION=$(grep '^version' "$REPO_ROOT/Cargo.toml" | head -1 | sed 's/.*= *"\(.*\)"/\1/')
MAGPIE_DIR="$REPO_ROOT/third_party/magpie"

rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources/data"
mkdir -p "$APP_BUNDLE/Contents/Resources/resources"
mkdir -p "$DIST_DIR"

MC_BIN="$REPO_ROOT/target/$TARGET/release/missioncenter"
if [[ ! -x "$MC_BIN" ]]; then
  MC_BIN="$REPO_ROOT/target/release/missioncenter"
fi
cp "$MC_BIN" "$APP_BUNDLE/Contents/MacOS/missioncenter"

# Magpie is looked up as missioncenter-magpie on PATH
if [[ -x "$BUILD_DIR/missioncenter-magpie" ]]; then
  cp "$BUILD_DIR/missioncenter-magpie" "$APP_BUNDLE/Contents/MacOS/missioncenter-magpie"
elif [[ -x "$MAGPIE_DIR/target/$TARGET/release/magpie" ]]; then
  cp "$MAGPIE_DIR/target/$TARGET/release/magpie" "$APP_BUNDLE/Contents/MacOS/missioncenter-magpie"
elif [[ -x "$MAGPIE_DIR/target/release/magpie" ]]; then
  cp "$MAGPIE_DIR/target/release/magpie" "$APP_BUNDLE/Contents/MacOS/missioncenter-magpie"
else
  echo "error: missioncenter-magpie / magpie release binary not found" >&2
  exit 1
fi

cp "$BUILD_DIR/resources/missioncenter.gresource" \
  "$APP_BUNDLE/Contents/Resources/resources/missioncenter.gresource"

cp "$BUILD_DIR/data/gschemas.compiled" \
  "$APP_BUNDLE/Contents/Resources/data/gschemas.compiled"

if [[ -f "$MAGPIE_DIR/platform-linux/hwdb/hw.db" ]]; then
  cp "$MAGPIE_DIR/platform-linux/hwdb/hw.db" \
    "$APP_BUNDLE/Contents/Resources/data/hw.db"
fi

cp "$BUILD_DIR/MissionCenter.icns" \
  "$APP_BUNDLE/Contents/Resources/MissionCenter.icns"

sed \
  -e "s|@VERSION@|$VERSION|g" \
  "$REPO_ROOT/scripts/macos/Info.plist.in" \
  > "$APP_BUNDLE/Contents/Info.plist"

cat > "$APP_BUNDLE/Contents/MacOS/launch.sh" << 'LAUNCHER'
#!/bin/bash
BUNDLE_DIR="$(cd "$(dirname "$0")/.." && pwd)"
export GSETTINGS_SCHEMA_DIR="$BUNDLE_DIR/Resources/data"
export MC_RESOURCE_DIR="$BUNDLE_DIR/Resources/resources"
if [[ -f "$BUNDLE_DIR/Resources/data/hw.db" ]]; then
  export MC_MAGPIE_HW_DB="$BUNDLE_DIR/Resources/data/hw.db"
fi
export PATH="$BUNDLE_DIR/MacOS:$PATH"
export XDG_DATA_DIRS="/opt/homebrew/share:/usr/local/share:/usr/share${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}"
export GDK_PIXBUF_MODULE_FILE="${GDK_PIXBUF_MODULE_FILE:-/opt/homebrew/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache}"
export GIO_MODULE_DIR="${GIO_MODULE_DIR:-/opt/homebrew/lib/gio/modules}"
exec "$BUNDLE_DIR/MacOS/missioncenter"
LAUNCHER
chmod +x "$APP_BUNDLE/Contents/MacOS/launch.sh"

plutil -replace CFBundleExecutable -string "launch.sh" "$APP_BUNDLE/Contents/Info.plist"

chmod +x "$APP_BUNDLE/Contents/MacOS/missioncenter"
chmod +x "$APP_BUNDLE/Contents/MacOS/missioncenter-magpie"

DMG_PATH="$DIST_DIR/$APP_NAME.dmg"
rm -f "$DMG_PATH"

create-dmg \
  --volname "Mission Center $VERSION" \
  --volicon "$BUILD_DIR/MissionCenter.icns" \
  --window-pos 200 120 \
  --window-size 600 400 \
  --icon-size 100 \
  --icon "$APP_NAME.app" 175 190 \
  --hide-extension "$APP_NAME.app" \
  --app-drop-link 425 190 \
  "$DMG_PATH" \
  "$DIST_DIR/"

echo "DMG created: $DMG_PATH"
