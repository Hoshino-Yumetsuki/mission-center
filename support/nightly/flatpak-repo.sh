#!/bin/sh

# flatpak-repo.sh
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

if [ -z "$NIGHTLY_FLATPAK_GPG_KEY" ]; then
    echo "NIGHTLY_FLATPAK_GPG_KEY is not set" >&2
    exit 1
fi

set -eu

REPO_URL="https://mission-center-devs.gitlab.io/mission-center/repo/"
HOMEPAGE="https://missioncenter.io"
TITLE="Mission Center Nightly"

GNUPGHOME="$(mktemp -d)"
export GNUPGHOME
echo "$NIGHTLY_FLATPAK_GPG_KEY" | base64 -d | gpg --batch --import
FINGERPRINT="$(gpg --list-secret-keys --with-colons | grep -m 1 '^fpr:' | cut -d: -f10)"
GPG_KEY="$(gpg --export | base64 -w0)"

mkdir -p public
ostree --repo=public/repo init --mode=archive-z2
for arch in x86_64 aarch64; do
    for bundle in "flatpak/missioncenter-$arch.flatpak" "flatpak/missioncenter-locale-$arch.flatpak"; do
        flatpak build-import-bundle --no-update-summary --gpg-sign="$FINGERPRINT" public/repo "$bundle"
    done
done

flatpak build-update-repo --gpg-sign="$FINGERPRINT" --generate-static-deltas --title="$TITLE" \
    --default-branch=nightly --homepage="$HOMEPAGE" public/repo

cat > public/missioncenter-nightly.flatpakrepo << EOF
[Flatpak Repo]
Title=$TITLE
Url=$REPO_URL
Homepage=$HOMEPAGE
DefaultBranch=nightly
GPGKey=$GPG_KEY
EOF

cat > public/missioncenter-nightly.flatpakref << EOF
[Flatpak Ref]
Title=$TITLE
Name=io.missioncenter.MissionCenter
Branch=nightly
Url=$REPO_URL
SuggestRemoteName=missioncenter-nightly
Homepage=$HOMEPAGE
RuntimeRepo=https://dl.flathub.org/repo/flathub.flatpakrepo
IsRuntime=false
GPGKey=$GPG_KEY
EOF
