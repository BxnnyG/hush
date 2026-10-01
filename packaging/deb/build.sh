#!/usr/bin/env bash
# Build a .deb from already built binaries.   packaging/deb/build.sh <version> [arch] [outdir]
set -euo pipefail
ver=${1:?version}
arch=${2:-amd64}
out=${3:-.}
here=$(cd "$(dirname "$0")/../.." && pwd)
root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT

"$here/packaging/stage.sh" "$root" /usr "$(cd "${BINDIR:-$here/target/release}" && pwd)"
mkdir -p "$root/DEBIAN"
size=$(du -sk "$root/usr" | cut -f1)
cat > "$root/DEBIAN/control" <<CTRL
Package: bxy-sordino
Version: $ver
Section: sound
Priority: optional
Architecture: $arch
Installed-Size: $size
Maintainer: BxnnyG <102485316+BxnnyG@users.noreply.github.com>
Conflicts: bxy-hush
Replaces: bxy-hush
Homepage: https://github.com/BxnnyG/sordino
Depends: pipewire, wireplumber, libwebkit2gtk-4.1-0, libgtk-3-0, libayatana-appindicator3-1, xdg-utils
Description: Virtual microphone with AI noise suppression for PipeWire
 Sordino adds a virtual microphone called "Sordino Mic" that removes background noise
 with DeepFilterNet 3, can add a light studio polish and experimental echo
 suppression. Pick "Sordino Mic" in Discord, Element, Teams, Zoom or OBS.
 .
 Sordino by BxnnyG, https://github.com/BxnnyG/sordino
CTRL
dpkg-deb --build --root-owner-group "$root" "$out/bxy-sordino_${ver}_${arch}.deb" >/dev/null
echo "$out/bxy-sordino_${ver}_${arch}.deb"
