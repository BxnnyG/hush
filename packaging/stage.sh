#!/usr/bin/env bash
# Stage the installed file layout of Hush into a directory, for the deb/rpm/tarball builders.
#   packaging/stage.sh <destdir> [prefix] [bindir-with-binaries]
set -euo pipefail
dest=${1:?destdir}
prefix=${2:-/usr}
bins=${3:-target/release}
here=$(cd "$(dirname "$0")/.." && pwd)

install -Dm755 "$bins/hushd"   "$dest$prefix/bin/hushd"
install -Dm755 "$bins/hushctl" "$dest$prefix/bin/hushctl"
install -Dm755 "$bins/hush"    "$dest$prefix/bin/hush"

# Desktop entry, D-Bus activation and systemd user unit point at the real install prefix.
install -Dm644 "$here/dist/io.github.bxnnyg.Hush.desktop" "$dest$prefix/share/applications/io.github.bxnnyg.Hush.desktop"
sed "s|^Exec=.*|Exec=$prefix/bin/hushd|" "$here/dist/io.github.bxnnyg.Hush.service" \
  | install -Dm644 /dev/stdin "$dest$prefix/share/dbus-1/services/io.github.bxnnyg.Hush.service"
sed "s|^ExecStart=.*|ExecStart=$prefix/bin/hushd|" "$here/dist/hushd.service" \
  | install -Dm644 /dev/stdin "$dest$prefix/lib/systemd/user/hushd.service"

install -Dm644 "$here/assets/hush.svg" "$dest$prefix/share/icons/hicolor/scalable/apps/io.github.bxnnyg.Hush.svg"
install -Dm644 "$here/ui/src-tauri/icons/128x128.png" "$dest$prefix/share/icons/hicolor/128x128/apps/io.github.bxnnyg.Hush.png"
install -Dm644 "$here/LICENSE" "$dest$prefix/share/licenses/bxy-hush/LICENSE"
install -Dm644 "$here/NOTICE"  "$dest$prefix/share/licenses/bxy-hush/NOTICE"
