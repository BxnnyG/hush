#!/usr/bin/env bash
# Build and install Sordino as a real pacman package from this checkout (Arch, CachyOS, Manjaro, ...).
#   packaging/install-arch.sh            build and install (asks for sudo)
#   NOINSTALL=1 packaging/install-arch.sh   only build, print the package path
# Remove again with: sudo pacman -R bxy-sordino
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' "$here/Cargo.toml" | head -1)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Package exactly what is in the working tree (committed files only, like a release would).
git -C "$here" ls-files -z | tar -C "$here" --null -T - --transform "s|^|sordino-$ver/|" -czf "$work/src.tar.gz"
cp "$here/packaging/aur/PKGBUILD" "$work/"
cd "$work"
args=(--nocheck --noconfirm)
[ -z "${NOINSTALL:-}" ] && args+=(--install)
SORDINO_SOURCE_URL="file://$work/src.tar.gz" makepkg "${args[@]}"
mkdir -p "$here/target/pkg"
cp "$work"/*.pkg.tar.zst "$here/target/pkg/"
echo "Package: $(ls "$here"/target/pkg/*.pkg.tar.zst | tail -1)"
