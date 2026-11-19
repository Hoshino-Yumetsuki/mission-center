#!/bin/sh

# publish.sh
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

# Uploads the nightly packages from the job artifacts under fixed names, then version.txt.

set -eu

single_file() {
    if [ "$#" -ne 1 ] || [ ! -f "$1" ]; then
        echo "Expected exactly one file, found: $*" >&2
        exit 1
    fi
    printf '%s' "$1"
}

upload() {
    echo "Uploading $1 as $2"
    curl --fail-with-body -sS --retry 3 -H "JOB-TOKEN: $CI_JOB_TOKEN" --upload-file "$1" \
        "$CI_API_V4_URL/projects/$CI_PROJECT_ID/packages/generic/missioncenter/nightly/$2"
    echo
}

SNAP_AMD64=$(single_file mission-center_*_amd64.snap)
SNAP_ARM64=$(single_file mission-center_*_arm64.snap)

PACKAGES="\
MissionCenter-nightly-x86_64.AppImage         MissionCenter-nightly-x86_64.AppImage
MissionCenter-nightly-x86_64.AppImage.zsync   MissionCenter-nightly-x86_64.AppImage.zsync
MissionCenter-nightly-aarch64.AppImage        MissionCenter-nightly-aarch64.AppImage
MissionCenter-nightly-aarch64.AppImage.zsync  MissionCenter-nightly-aarch64.AppImage.zsync
flatpak/missioncenter-x86_64.flatpak          missioncenter-nightly-x86_64.flatpak
flatpak/missioncenter-aarch64.flatpak         missioncenter-nightly-aarch64.flatpak
$SNAP_AMD64 mission-center-nightly_amd64.snap
$SNAP_ARM64 mission-center-nightly_arm64.snap"

echo "$PACKAGES" | while read -r artifact name; do single_file "$artifact" > /dev/null; done
echo "$PACKAGES" | while read -r artifact name; do upload "$artifact" "$name"; done

printf '%s\n' "$VERSION_STRING" > version.txt
upload version.txt version.txt
