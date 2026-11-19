#!/bin/sh

# create-appimage.sh
#
# Copyright 2026 Mission Center Developers
#
# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <http://www.gnu.org/licenses/>.
#
# SPDX-License-Identifier: GPL-3.0-or-later

set -eux

export SRC_PATH=${SRC_PATH:-"/hostfs"}

unset DISPLAY
unset WAYLAND_DISPLAY

ARCH="$(uname -m)"
SHARUN="https://raw.githubusercontent.com/pkgforge-dev/Anylinux-AppImages/refs/heads/main/useful-tools/quick-sharun.sh"
EXTRA_PACKAGES="https://raw.githubusercontent.com/pkgforge-dev/Anylinux-AppImages/refs/heads/main/useful-tools/get-debloated-pkgs.sh"

pacman -Syu --noconfirm base-devel zsync wget gtk4 libadwaita patchelf

wget "$SHARUN" -O ./quick-sharun
chmod +x ./quick-sharun

wget "$EXTRA_PACKAGES" -O ./get-debloated-pkgs
chmod +x ./get-debloated-pkgs

export VERSION="${VERSION_STRING:-$(grep -oP 'version = \K.*' "$SRC_PATH/Cargo.toml" | head -n1 | tr -d '"')}"
export ICON=/usr/share/icons/hicolor/scalable/apps/io.missioncenter.MissionCenter.svg
export DESKTOP=/usr/share/applications/io.missioncenter.MissionCenter.desktop
export OUTPATH="$SRC_PATH/"

if [ "${APPIMAGE_NIGHTLY:-false}" = "true" ]; then
    export OUTNAME="MissionCenter-nightly-$ARCH.AppImage"
    NIGHTLY_URL="https://gitlab.com/api/v4/projects/44426042/packages/generic/missioncenter/nightly"
    APPIMAGE_URL="${APPIMAGE_URL:-$NIGHTLY_URL/$OUTNAME}"
    ZSYNC_URL="${ZSYNC_URL:-$NIGHTLY_URL/$OUTNAME.zsync}"
else
    export OUTNAME="MissionCenter-$VERSION-$ARCH.AppImage"
    RELEASES_URL="https://gitlab.com/mission-center-devs/mission-center/-/releases"
    APPIMAGE_URL="${APPIMAGE_URL:-$RELEASES_URL/v$VERSION/downloads/$OUTNAME}"
    ZSYNC_URL="${ZSYNC_URL:-$RELEASES_URL/permalink/latest/downloads/MissionCenter-$ARCH.AppImage.zsync}"
fi
export UPINFO="zsync|$ZSYNC_URL"

./get-debloated-pkgs --add-common --prefer-nano

./quick-sharun /usr/bin/missioncenter /usr/bin/missioncenter-magpie
./quick-sharun --make-appimage

cd "$OUTPATH"
zsyncmake -u "$APPIMAGE_URL" -o "$OUTNAME.zsync" "$OUTNAME"
