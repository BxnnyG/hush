#!/usr/bin/env bash
# Build an .rpm from already built binaries.   packaging/rpm/build.sh <version> [outdir]
set -euo pipefail
ver=${1:?version}
out=${2:-.}
here=$(cd "$(dirname "$0")/../.." && pwd)
top=$(mktemp -d)
trap 'rm -rf "$top"' EXIT
rpmbuild -bb "$here/packaging/rpm/bxy-hush.spec" \
  --define "_topdir $top" \
  --define "hush_version $ver" \
  --define "hush_stage $here/packaging/stage.sh" \
  --define "hush_bins $(cd "${BINDIR:-$here/target/release}" && pwd)" >/dev/null
mkdir -p "$out"
cp "$top"/RPMS/*/*.rpm "$out"/
ls "$out"/*.rpm
