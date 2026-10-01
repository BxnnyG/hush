#!/usr/bin/env bash
# Stage the installed file layout of Sordino into a directory, for the deb/rpm/tarball builders.
#   packaging/stage.sh <destdir> [prefix] [bindir-with-binaries]
set -euo pipefail
dest=${1:?destdir}
prefix=${2:-/usr}
bins=${3:-target/release}
here=$(cd "$(dirname "$0")/.." && pwd)

install -Dm755 "$bins/sordinod"   "$dest$prefix/bin/sordinod"
install -Dm755 "$bins/sordinoctl" "$dest$prefix/bin/sordinoctl"
install -Dm755 "$bins/sordino"    "$dest$prefix/bin/sordino"

# Desktop entry, D-Bus activation and systemd user unit point at the real install prefix.
install -Dm644 "$here/dist/io.github.bxnnyg.Sordino.desktop" "$dest$prefix/share/applications/io.github.bxnnyg.Sordino.desktop"
sed "s|^Exec=.*|Exec=$prefix/bin/sordinod|" "$here/dist/io.github.bxnnyg.Sordino.service" \
  | install -Dm644 /dev/stdin "$dest$prefix/share/dbus-1/services/io.github.bxnnyg.Sordino.service"
sed "s|^ExecStart=.*|ExecStart=$prefix/bin/sordinod|" "$here/dist/sordinod.service" \
  | install -Dm644 /dev/stdin "$dest$prefix/lib/systemd/user/sordinod.service"

install -Dm644 "$here/dist/io.github.bxnnyg.Sordino.metainfo.xml" "$dest$prefix/share/metainfo/io.github.bxnnyg.Sordino.metainfo.xml"
install -Dm644 "$here/assets/sordino.svg" "$dest$prefix/share/icons/hicolor/scalable/apps/io.github.bxnnyg.Sordino.svg"
install -Dm644 "$here/ui/src-tauri/icons/128x128.png" "$dest$prefix/share/icons/hicolor/128x128/apps/io.github.bxnnyg.Sordino.png"
install -Dm644 "$here/LICENSE" "$dest$prefix/share/licenses/bxy-sordino/LICENSE"
install -Dm644 "$here/NOTICE"  "$dest$prefix/share/licenses/bxy-sordino/NOTICE"
