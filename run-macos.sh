#!/bin/bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "$0")" && pwd)"
BUILD_DIR="$PROJECT_ROOT/build-macos"
TARGET="${CARGO_BUILD_TARGET:-aarch64-apple-darwin}"
BLUEPRINT_COMPILER="${BLUEPRINT_COMPILER:-$(command -v blueprint-compiler || true)}"
if [[ -z "$BLUEPRINT_COMPILER" && -x /opt/homebrew/bin/blueprint-compiler ]]; then
  BLUEPRINT_COMPILER="/opt/homebrew/bin/blueprint-compiler"
fi
if [[ -z "$BLUEPRINT_COMPILER" ]]; then
  echo "error: blueprint-compiler not found (brew install blueprint-compiler)" >&2
  exit 1
fi

MAGPIE_DIR="$PROJECT_ROOT/third_party/magpie"
if [[ ! -d "$MAGPIE_DIR" ]]; then
  echo "error: magpie missing at third_party/magpie (subprojects/magpie -> ../third_party/magpie)" >&2
  exit 1
fi

echo "==> Building magpie ($TARGET)..."
(
  cd "$MAGPIE_DIR"
  cargo build --target "$TARGET"
)

MAGPIE_BIN="$MAGPIE_DIR/target/$TARGET/debug/magpie"
if [[ ! -x "$MAGPIE_BIN" ]]; then
  # Host triple build (no --target) falls back here
  MAGPIE_BIN="$MAGPIE_DIR/target/debug/magpie"
fi
if [[ ! -x "$MAGPIE_BIN" ]]; then
  echo "error: magpie binary not found after build" >&2
  exit 1
fi

echo "==> Generating config.rs..."
mkdir -p "$BUILD_DIR/src"
VERSION=$(grep '^version' "$PROJECT_ROOT/Cargo.toml" | head -1 | sed 's/.*= *"\(.*\)"/\1/')
sed \
  -e "s|@VERSION@|\"$VERSION\"|g" \
  -e 's|@GETTEXT_PACKAGE@|"missioncenter"|g' \
  -e 's|@LOCALEDIR@|""|g' \
  -e 's|@PKGDATADIR@|""|g' \
  -e 's|@APP_ID@|"io.missioncenter.MissionCenter"|g' \
  "$PROJECT_ROOT/src/config.rs.in" > "$BUILD_DIR/src/config.rs"

echo "==> Building missioncenter..."
cd "$PROJECT_ROOT"
BUILD_ROOT="$BUILD_DIR" cargo build --target "$TARGET"

MC_BIN="$PROJECT_ROOT/target/$TARGET/debug/missioncenter"
if [[ ! -x "$MC_BIN" ]]; then
  MC_BIN="$PROJECT_ROOT/target/debug/missioncenter"
fi
if [[ ! -x "$MC_BIN" ]]; then
  echo "error: missioncenter binary not found after build" >&2
  exit 1
fi

echo "==> Compiling Blueprint UI files..."
mkdir -p "$BUILD_DIR/resources"
find "$PROJECT_ROOT/resources/ui" -name "*.blp" | while read -r blp; do
  rel="${blp#$PROJECT_ROOT/resources/}"
  out="$BUILD_DIR/resources/${rel%.blp}.ui"
  mkdir -p "$(dirname "$out")"
  "$BLUEPRINT_COMPILER" compile --output "$out" "$blp"
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

echo "==> Staging magpie as missioncenter-magpie..."
cp "$MAGPIE_BIN" "$BUILD_DIR/missioncenter-magpie"
chmod +x "$BUILD_DIR/missioncenter-magpie"

echo "==> Launching Mission Center..."
export PATH="$BUILD_DIR:$PATH"
export GSETTINGS_SCHEMA_DIR="$BUILD_DIR/data"
export MC_RESOURCE_DIR="$BUILD_DIR/resources"
# Optional; only used by Linux hwdb lookups if present
if [[ -f "$MAGPIE_DIR/platform-linux/hwdb/hw.db" ]]; then
  export MC_MAGPIE_HW_DB="$MAGPIE_DIR/platform-linux/hwdb/hw.db"
fi

cd "$PROJECT_ROOT"
exec "$MC_BIN"
