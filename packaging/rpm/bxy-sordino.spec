# Packages already built binaries. Build with packaging/rpm/build.sh
Name:           bxy-sordino
Version:        %{sordino_version}
Release:        1
Summary:        Virtual microphone with AI noise suppression for PipeWire
License:        GPL-3.0-or-later
URL:            https://github.com/BxnnyG/sordino
Requires:       pipewire
Requires:       wireplumber
Requires:       xdg-utils
# Binaries are prebuilt; do not strip, debuginfo or guess dependencies from a foreign toolchain.
%global debug_package %{nil}
%global __os_install_post %{nil}

%description
Sordino adds a virtual microphone called "Sordino Mic" that removes background noise with
DeepFilterNet 3, can add a light studio polish and experimental echo suppression.
Pick "Sordino Mic" in Discord, Element, Teams, Zoom or OBS.

Sordino by BxnnyG, https://github.com/BxnnyG/sordino

%prep

%build

%install
%{sordino_stage} %{buildroot} /usr %{sordino_bins}

%files
/usr/bin/sordino
/usr/bin/sordinod
/usr/bin/sordinoctl
/usr/share/applications/io.github.bxnnyg.Sordino.desktop
/usr/share/dbus-1/services/io.github.bxnnyg.Sordino.service
/usr/lib/systemd/user/sordinod.service
/usr/share/metainfo/io.github.bxnnyg.Sordino.metainfo.xml
/usr/share/icons/hicolor/scalable/apps/io.github.bxnnyg.Sordino.svg
/usr/share/icons/hicolor/128x128/apps/io.github.bxnnyg.Sordino.png
/usr/share/licenses/bxy-sordino/
