<img float="left"  src="https://gitlab.com/mission-center-devs/mission-center/-/raw/main/data/icons/hicolor/scalable/apps/io.missioncenter.MissionCenter.svg" alt="drawing" width="64"/> 

# Mission Center

Monitor your CPU, Memory, Disk, Network and GPU usage with [Mission Center](https://missioncenter.io/)

![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0001-cpu.png)

## Features

* Monitor overall or per-thread CPU usage
* See system process, thread, and handle count, uptime, clock speed (base and current), cache sizes
* Monitor RAM and Swap usage
* See a breakdown of how the memory is being used by the system
* Monitor Disk utilization and transfer rates
* Monitor network utilization and transfer speeds
* See network interface information such as network card name, connection type (Wi-Fi or Ethernet), wireless speeds and
  frequency, hardware address, IP address
* Monitor overall GPU usage, video encoder and decoder usage, memory usage and power consumption, powered by the popular
  NVTOP project
* Monitor system fans
* Monitor battery charge and see detailed battery information
* See a breakdown of resource usage by app and process
* View and control system services using systemd or OpenRC
* Supports a minified summary view for simple monitoring
* Use hardware accelerated rendering for all the graphs in an effort to reduce CPU and overall resource usage

## Limitations

Please note there is ongoing work to overcome all of these.

* Per-process network monitoring requires manual setup,
  see [this page](https://gitlab.com/mission-center-devs/mission-center/-/wikis/Home/Nethogs) for more information.
* Intel GPU monitoring is only supported for Broadwell and later GPUs.

Please also note that as Mission Center is a libadwaita application, it will not follow system-defined stylesheets
(themes).

## Installing

[AppImage (x86_64)](https://gitlab.com/mission-center-devs/mission-center/-/jobs/15536631699/artifacts/raw/MissionCenter-1.2.0-x86_64.AppImage)  
[AppImage (ARM64)](https://gitlab.com/mission-center-devs/mission-center/-/jobs/15536631700/artifacts/raw/MissionCenter-1.2.0-aarch64.AppImage)  
[Flatpak](https://flathub.org/apps/io.missioncenter.MissionCenter)  
[Snap](https://snapcraft.io/mission-center)

Also available from https://portable-linux-apps.github.io/apps/mission-center.html

Might also be available in your distribution's repository:  
[![](https://repology.org/badge/vertical-allrepos/mission-center.svg)](https://repology.org/project/mission-center/versions)

Installed by default in:

* [Aurora](https://getaurora.dev/)
* [Bazzite](https://bazzite.gg)
* [Bluefin](https://projectbluefin.io/)
* [DeLinuxCo](https://www.delinuxco.com/)

Source code is available at [GitLab](https://gitlab.com/mission-center-devs/mission-center)

## Screenshots

<details>
  <summary><b>Show</b></summary>

  <br/>

*CPU view*  
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0001-cpu.png)

*Memory view*  
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0002-memory.png)

*Disk view*  
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0003-disk.png)

*Ethernet and Wi-Fi view*  
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0004-ethernet.png)
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0005-wifi.png)

*GPU view*  
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0006-gpu.png)

*Fan view*  
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0007-fan.png)

*Apps page*  
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0008-apps.png)

*Services page*  
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0008-services.png)

*Dark mode*  
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0009-cpu-dark.png)
![](https://gitlab.com/mission-center-devs/mission-center/-/raw/main/screenshots/0010-disk-dark.png)

  </details>

## Building

See [BUILD.md](BUILD.md) for instructions on building Mission Center.

## Contributing

### Issues

Report issues to GitLab [issue tracking system](https://gitlab.com/mission-center-devs/mission-center/-/issues).

### Discord

Join [the Discord server](https://discord.gg/RG7QTeB9yk) and let's talk about what you think is missing or can be
improved.

### Translations

If you'd like to help translating Mission Center into your language, please head over
to [Weblate](https://hosted.weblate.org/engage/mission-center/).

<a href="https://hosted.weblate.org/engage/mission-center/">
  <img src="https://hosted.weblate.org/widgets/mission-center/-/mission-center/multi-auto.svg" alt="Translation status" />
</a>

### Monetary Contributions

Instead of donating to Mission Center directly, consider supporting the projects that Mission Center depends on:

* [GNOME](https://donate.gnome.org/)
* [NNG](https://github.com/gdamore)
* [NVTOP](https://github.com/Syllo/nvtop)
* [Rust Foundation](https://rustfoundation.org/get-involved/)

If you'd, still, like to support the development of Mission Center financially, please visit
our [Open Collective page](https://opencollective.com/mission-center).

Comments, suggestions, bug reports and contributions are welcome.

## License

This program is free software; you can redistribute it and/or modify it under the terms of the GNU General Public
License as published by the Free Software Foundation; either version 3 of the License, or (at your option) any later
version.

Please see COPYING file in the root of this repository for the complete license text. Alternatively see
[the official license](https://www.gnu.org/licenses/gpl-3.0.html) as written by the Free Software Foundation.

## Code of Conduct

Mission Center follows the GNOME Code of Conduct. All communications in project spaces are expected to follow it.