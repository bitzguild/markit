#!/bin/sh
# Package the built Markit.app into a compressed DMG with hdiutil.
# (Tauri's own DMG step drives Finder via AppleScript, which fails without
# Automation permission; this needs none.)
set -eu
cd "$(dirname "$0")/../src-tauri/target/release/bundle"
VERSION=$(plutil -extract CFBundleShortVersionString raw macos/Markit.app/Contents/Info.plist)
ARCH=$(uname -m)
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
cp -R macos/Markit.app "$STAGE/"
ln -s /Applications "$STAGE/Applications"
mkdir -p dmg
OUT="dmg/Markit_${VERSION}_${ARCH}.dmg"
hdiutil create -volname Markit -srcfolder "$STAGE" -ov -format UDZO "$OUT" >/dev/null
echo "Created $(pwd)/$OUT"
