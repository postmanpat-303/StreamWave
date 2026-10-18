#!/usr/bin/env bash
# Universal (Apple Silicon + Intel) StreamWave.app, ad-hoc signed (not notarised), packaged as
#   dist/StreamWave_<ver>_universal.dmg      (app + Applications link + read-me)
#   dist/StreamWave_<ver>_universal.app.zip  (the .app, zipped with ditto)
# and, when TAURI_SIGNING_PRIVATE_KEY is set, the signed updater bundle
#   dist/StreamWave_<ver>_universal.app.tar.gz + .sig  and  dist/latest-darwin.json
# macOS only. Used by .github/workflows/release.yml (job "macos").
set -euo pipefail
cd "$(dirname "$0")/.."
VER=$(node -p "require('./src-tauri/tauri.conf.json').version")
export MACOSX_DEPLOYMENT_TARGET=11.0
ARGS=(--target universal-apple-darwin)
if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  ARGS+=(--config src-tauri/tauri.updater.conf.json)
else
  echo "::warning::TAURI_SIGNING_PRIVATE_KEY is not set: building without the signed updater bundle (.app.tar.gz/.sig), so latest.json gets no macOS entry"
  unset TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD || true
fi
npm run tauri build -- "${ARGS[@]}"

B=src-tauri/target/universal-apple-darwin/release/bundle/macos
APP="$B/StreamWave.app"
test -d "$APP"
lipo -archs "$APP/Contents/MacOS/StreamWave"
# ad-hoc signed by the bundler (bundle.macOS.signingIdentity "-" in tauri.macos.conf.json), so Apple Silicon runs it
codesign --verify --deep --strict --verbose=2 "$APP"
plutil -p "$APP/Contents/Info.plist" | grep -E "CFBundleShortVersionString|NSMicrophoneUsageDescription|LSMinimumSystemVersion"

OUT=dist; rm -rf "$OUT"; mkdir -p "$OUT"
NAME="StreamWave_${VER}_universal"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$OUT/$NAME.app.zip"

DMGDIR="$(mktemp -d)"
ditto "$APP" "$DMGDIR/StreamWave.app"
ln -s /Applications "$DMGDIR/Applications"
cat > "$DMGDIR/Read me first.txt" <<TXT
StreamWave $VER for macOS (Apple Silicon and Intel)

1. Drag StreamWave into Applications.
2. The first time, right-click (or Control-click) StreamWave in Applications and choose Open,
   then click Open again. StreamWave isn't notarised by Apple yet, so a normal double-click
   is blocked the first time. After that it opens normally.
3. Allow microphone access when macOS asks.

For MP3/FLAC/Opus export and OBS video import, install FFmpeg (for example: brew install ffmpeg).
https://github.com/postmanpat-303/StreamWave
TXT
# hdiutil sometimes fails with "Resource busy" on CI runners; try a few times.
ok=0
for i in 1 2 3 4 5; do
  if hdiutil create -volname "StreamWave $VER" -srcfolder "$DMGDIR" -fs HFS+ -format UDZO -ov "$OUT/$NAME.dmg"; then ok=1; break; fi
  echo "hdiutil failed (try $i), retrying"; sleep $((i * 5))
done
[ "$ok" = 1 ] && test -f "$OUT/$NAME.dmg"
rm -rf "$DMGDIR"

TGZ=$(ls "$B"/*.app.tar.gz 2>/dev/null | head -1 || true)
if [ -n "${TGZ}" ] && [ -f "$TGZ.sig" ]; then
  cp "$TGZ" "$OUT/$NAME.app.tar.gz"; cp "$TGZ.sig" "$OUT/$NAME.app.tar.gz.sig"
  node -e '
    const fs = require("fs"); const [ver, name] = process.argv.slice(1);
    const e = { signature: fs.readFileSync(`dist/${name}.app.tar.gz.sig`, "utf8").trim(),
                url: `https://github.com/postmanpat-303/StreamWave/releases/download/v${ver}/${name}.app.tar.gz` };
    fs.writeFileSync("dist/latest-darwin.json", JSON.stringify({ version: ver, platforms: { "darwin-universal": e, "darwin-aarch64": e, "darwin-x86_64": e } }, null, 2));
  ' "$VER" "$NAME"
fi
(cd "$OUT" && shasum -a 256 $NAME.* > SHA256SUMS-macos.txt && cat SHA256SUMS-macos.txt)
