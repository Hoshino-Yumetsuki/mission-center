#!/bin/bash
set -e

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BUILD_DIR="$REPO_ROOT/build-macos"
DIST_DIR="$REPO_ROOT/dist"
APP_NAME="MissionCenter"
APP_BUNDLE="$DIST_DIR/$APP_NAME.app"
VERSION=$(grep '^version' "$REPO_ROOT/Cargo.toml" | head -1 | sed 's/.*= *"\(.*\)"/\1/')

rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"
mkdir -p "$APP_BUNDLE/Contents/Resources/data"
mkdir -p "$APP_BUNDLE/Contents/Resources/resources"
mkdir -p "$DIST_DIR"

cp "$REPO_ROOT/target/aarch64-apple-darwin/release/missioncenter" \
   "$APP_BUNDLE/Contents/MacOS/missioncenter"

cp "$BUILD_DIR/missioncenter-gatherer" \
   "$APP_BUNDLE/Contents/MacOS/missioncenter-gatherer" 2>/dev/null || \
cp "$REPO_ROOT/src/sys_info_v2/gatherer/target/aarch64-apple-darwin/release/gatherer" \
   "$APP_BUNDLE/Contents/MacOS/missioncenter-gatherer"

cp "$BUILD_DIR/resources/missioncenter.gresource" \
   "$APP_BUNDLE/Contents/Resources/resources/missioncenter.gresource"

cp "$BUILD_DIR/data/gschemas.compiled" \
   "$APP_BUNDLE/Contents/Resources/data/gschemas.compiled"

cp -r "$REPO_ROOT/data/hwdb" \
   "$APP_BUNDLE/Contents/Resources/data/hwdb" 2>/dev/null || true

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
export HW_DB_DIR="$BUNDLE_DIR/Resources/data/hwdb"
export MC_GATHERER_SOCKET="/tmp/missioncenter-gatherer-$$.sock"
export PATH="$BUNDLE_DIR/MacOS:$PATH"
export XDG_DATA_DIRS="/opt/homebrew/share:/usr/local/share:/usr/share"
export GDK_PIXBUF_MODULE_FILE="/opt/homebrew/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"
export GIO_MODULE_DIR="/opt/homebrew/lib/gio/modules"
exec "$BUNDLE_DIR/MacOS/missioncenter"
LAUNCHER
chmod +x "$APP_BUNDLE/Contents/MacOS/launch.sh"

plutil -replace CFBundleExecutable -string "launch.sh" "$APP_BUNDLE/Contents/Info.plist"

chmod +x "$APP_BUNDLE/Contents/MacOS/missioncenter"
chmod +x "$APP_BUNDLE/Contents/MacOS/missioncenter-gatherer"

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
