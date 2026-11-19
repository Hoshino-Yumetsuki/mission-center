#!/usr/bin/env python3

# flatpak-manifest.py
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

import json
import sys


def main():
    if len(sys.argv) != 4:
        sys.exit(f"Usage: {sys.argv[0]} <manifest> <version> <output>")

    manifest_path, version, output_path = sys.argv[1:]

    with open(manifest_path) as f:
        manifest = json.load(f)

    app_module = next(
        (m for m in manifest["modules"] if isinstance(m, dict) and m.get("name") == "missioncenter"),
        None,
    )
    if app_module is None:
        sys.exit(f"No 'missioncenter' module in {manifest_path}")

    app_module["config-opts"].append(f"-Dversion={version}")

    with open(output_path, "w") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")


if __name__ == "__main__":
    main()
