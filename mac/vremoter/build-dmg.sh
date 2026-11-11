#!/bin/zsh
set -euo pipefail

SCRIPT_DIR="${0:A:h}"
PLIST="$SCRIPT_DIR/Packaging/Info.plist"
VERSION=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$PLIST")
STAGING="$SCRIPT_DIR/dist/dmg-root"
FINAL_DMG="$SCRIPT_DIR/dist/vRemoter-$VERSION.dmg"

"$SCRIPT_DIR/package-app.sh"

rm -rf "$STAGING" "$FINAL_DMG"
mkdir -p "$STAGING"
ditto "$SCRIPT_DIR/dist/build/vRemote.app" "$STAGING/vRemote.app"
ln -s /Applications "$STAGING/Applications"

hdiutil create \
  -volname "vRemoter $VERSION" \
  -srcfolder "$STAGING" \
  -ov \
  -format UDZO \
  "$FINAL_DMG"

rm -rf "$STAGING"
print "Built app-only update: $FINAL_DMG"
