#!/usr/bin/env bash
# Linux packages for a release, named like the other Wave apps (BigWave Radio, Wave Hub):
#   dist-linux/StreamWave-<ver>-linux-x86_64.deb          Debian / Ubuntu / Mint / Pop!_OS   (Tauri deb bundle)
#   dist-linux/StreamWave-<ver>-linux-x86_64.rpm          Fedora / openSUSE / RHEL           (Tauri rpm bundle)
#   dist-linux/StreamWave-<ver>-linux-x86_64.pkg.tar.zst  Arch / CachyOS / Manjaro / EndeavourOS
#                                                        (makepkg on packaging/arch/streamwave-bin/PKGBUILD, from the .deb)
# and, when TAURI_SIGNING_PRIVATE_KEY is set, the in-app updater target (its name is in latest.json, so it keeps Tauri's name):
#   dist-linux/StreamWave_<ver>_amd64.AppImage + .sig
# Build on Ubuntu 22.04 (glibc 2.35) or similar. Needs rpmbuild (rpm) and makepkg (Debian/Ubuntu: pacman-package-manager).
# .deb/.rpm/Arch installs update by hand: the app's update check links to the download page for them.
set -euo pipefail
cd "$(dirname "$0")/.."
VER=$(node -p "require('./src-tauri/tauri.conf.json').version")
grep -q "^pkgver=$VER$" packaging/arch/streamwave-bin/PKGBUILD || { echo "PKGBUILD pkgver is not $VER" >&2; exit 1; }
OUT=dist-linux; rm -rf "$OUT"; mkdir -p "$OUT"
B=src-tauri/target/release/bundle
N="StreamWave-$VER-linux-x86_64"

if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  npm run tauri -- build --bundles appimage --config src-tauri/tauri.updater.conf.json
  cp "$B/appimage/StreamWave_${VER}_amd64.AppImage" "$B/appimage/StreamWave_${VER}_amd64.AppImage.sig" "$OUT/"
else
  echo "TAURI_SIGNING_PRIVATE_KEY is not set: skipping the signed updater AppImage" >&2
fi

env -u TAURI_SIGNING_PRIVATE_KEY -u TAURI_SIGNING_PRIVATE_KEY_PASSWORD npm run tauri -- build --bundles deb,rpm
cp "$B/deb/StreamWave_${VER}_amd64.deb" "$OUT/$N.deb"
cp "$B/rpm/StreamWave-$VER-1.x86_64.rpm" "$OUT/$N.rpm"

W="$(mktemp -d)"
cp packaging/arch/streamwave-bin/PKGBUILD "$W/"
cp "$OUT/$N.deb" "$W/"             # makepkg uses these local copies instead of downloading
cp LICENSE "$W/LICENSE-$VER"
sed -e 's/^CARCH=.*/CARCH="x86_64"/' -e 's/^CHOST=.*/CHOST="x86_64-pc-linux-gnu"/' /etc/makepkg.conf > "$W/makepkg.conf"
echo 'PKGEXT=".pkg.tar.zst"' >> "$W/makepkg.conf"
(cd "$W" && makepkg --config "$W/makepkg.conf" -f --nodeps --noconfirm --skipinteg >/dev/null)
cp "$W/streamwave-bin-$VER-1-x86_64.pkg.tar.zst" "$OUT/$N.pkg.tar.zst"
rm -rf "$W"

(cd "$OUT" && sha256sum ./* | sed 's#  \./#  #' > SHA256SUMS-linux.txt && cat SHA256SUMS-linux.txt)
