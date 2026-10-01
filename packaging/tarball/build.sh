#!/usr/bin/env bash
# Portable tarball with an install script.   packaging/tarball/build.sh <version> [outdir]
set -euo pipefail
ver=${1:?version}
out=${2:-.}
here=$(cd "$(dirname "$0")/../.." && pwd)
name="bxy-sordino-$ver-x86_64-linux"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/$name"
"$here/packaging/stage.sh" "$work/$name/files" /usr "$(cd "${BINDIR:-$here/target/release}" && pwd)"
cat > "$work/$name/install.sh" <<'INST'
#!/usr/bin/env bash
# Install Sordino:  sudo ./install.sh [prefix]     (default prefix /usr/local)
#                ./install.sh ~/.local          (per user)
set -euo pipefail
prefix=${1:-/usr/local}
here=$(cd "$(dirname "$0")" && pwd)
cd "$here/files/usr"
find . -type f | while read -r f; do
  f=${f#./}
  mode=644; [ -x "$f" ] && mode=755
  install -Dm$mode "$f" "$prefix/$f"
done
sed -i "s|/usr/bin/sordinod|$prefix/bin/sordinod|" "$prefix/share/dbus-1/services/io.github.bxnnyg.Sordino.service" "$prefix/lib/systemd/user/sordinod.service" 2>/dev/null || true
echo "Installed Sordino to $prefix. Needs: pipewire, wireplumber, webkit2gtk-4.1, gtk3, libayatana-appindicator."
INST
chmod +x "$work/$name/install.sh"
cp "$here/README.md" "$here/LICENSE" "$here/NOTICE" "$work/$name/"
mkdir -p "$out"
tar -C "$work" -czf "$out/$name.tar.gz" "$name"
echo "$out/$name.tar.gz"
