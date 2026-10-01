# Packages already built binaries. Build with packaging/rpm/build.sh
Name:           bxy-hush
Version:        %{hush_version}
Release:        1
Summary:        Virtual microphone with AI noise suppression for PipeWire
License:        GPL-3.0-or-later
URL:            https://github.com/BxnnyG/hush
Requires:       pipewire
Requires:       wireplumber
Requires:       xdg-utils
# Binaries are prebuilt; do not strip, debuginfo or guess dependencies from a foreign toolchain.
%global debug_package %{nil}
%global __os_install_post %{nil}

%description
Hush adds a virtual microphone called "Hush Mic" that removes background noise with
DeepFilterNet 3, can add a light studio polish and experimental echo suppression.
Pick "Hush Mic" in Discord, Element, Teams, Zoom or OBS.

Hush by BxnnyG, https://github.com/BxnnyG/hush

%prep

%build

%install
%{hush_stage} %{buildroot} /usr %{hush_bins}

%files
/usr/bin/hush
/usr/bin/hushd
/usr/bin/hushctl
/usr/share/applications/io.github.bxnnyg.Hush.desktop
/usr/share/dbus-1/services/io.github.bxnnyg.Hush.service
/usr/lib/systemd/user/hushd.service
/usr/share/icons/hicolor/scalable/apps/io.github.bxnnyg.Hush.svg
/usr/share/icons/hicolor/128x128/apps/io.github.bxnnyg.Hush.png
/usr/share/licenses/bxy-hush/
