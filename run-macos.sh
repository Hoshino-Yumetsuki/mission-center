#!/bin/bash
set -e

PROJECT_ROOT="$(cd "$(dirname "$0")" && pwd)"
BUILD_DIR="$PROJECT_ROOT/build-macos"
BLUEPRINT_COMPILER="/opt/homebrew/bin/blueprint-compiler"

echo "==> Building gatherer..."
cd "$PROJECT_ROOT/src/sys_info_v2/gatherer"
cargo build --target aarch64-apple-darwin 2>&1 | grep -v "^warning"

echo "==> Building main app..."
cd "$PROJECT_ROOT"
cargo build 2>&1 | grep -v "^warning"

echo "==> Compiling Blueprint UI files..."
mkdir -p "$BUILD_DIR/resources"
find "$PROJECT_ROOT/resources/ui" -name "*.blp" | while read blp; do
    rel="${blp#$PROJECT_ROOT/resources/}"
    out="$BUILD_DIR/resources/${rel%.blp}.ui"
    mkdir -p "$(dirname "$out")"
    $BLUEPRINT_COMPILER compile --output "$out" "$blp" 2>/dev/null
done
echo "    Done"

echo "==> Compiling GSettings schema..."
mkdir -p "$BUILD_DIR/data"
glib-compile-schemas --targetdir="$BUILD_DIR/data" "$PROJECT_ROOT/data"

echo "==> Compiling gresource..."
cd "$PROJECT_ROOT/resources"
glib-compile-resources \
    --sourcedir="$BUILD_DIR/resources" \
    --sourcedir="$PROJECT_ROOT/resources" \
    --sourcedir="$PROJECT_ROOT/data" \
    --target="$BUILD_DIR/resources/missioncenter.gresource" \
    missioncenter.gresource.xml

echo "==> Copying gatherer binary..."
cp "$PROJECT_ROOT/src/sys_info_v2/gatherer/target/aarch64-apple-darwin/debug/gatherer" \
   "$BUILD_DIR/missioncenter-gatherer"

echo "==> Launching Mission Center..."
export PATH="$BUILD_DIR:$PATH"
export GSETTINGS_SCHEMA_DIR="$BUILD_DIR/data"
export MC_RESOURCE_DIR="$BUILD_DIR/resources"
export HW_DB_DIR="$PROJECT_ROOT/data"
export MC_GATHERER_SOCKET="/tmp/missioncenter-gatherer.sock"

cd "$PROJECT_ROOT"
exec "$PROJECT_ROOT/target/debug/missioncenter"
