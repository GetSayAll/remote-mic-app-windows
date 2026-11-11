#!/bin/zsh
set -euo pipefail

SCRIPT_DIR="${0:A:h}"
APP="$SCRIPT_DIR/dist/build/vRemote.app"
CONTENTS="$APP/Contents"

cd "$SCRIPT_DIR"
swift build -c release

rm -rf "$APP"
mkdir -p "$CONTENTS/MacOS"
mkdir -p "$CONTENTS/Resources"
cp "$SCRIPT_DIR/.build/release/vRemote" "$CONTENTS/MacOS/vRemote"
cp "$SCRIPT_DIR/Packaging/Info.plist" "$CONTENTS/Info.plist"
for resource_bundle in "$SCRIPT_DIR/.build/release/"*.bundle; do
  [[ -d "$resource_bundle" ]] || continue
  ditto "$resource_bundle" "$CONTENTS/Resources/${resource_bundle:t}"
done
for localization in "$SCRIPT_DIR/Packaging"/*.lproj; do
  [[ -d "$localization" ]] || continue
  ditto "$localization" "$CONTENTS/Resources/${localization:t}"
done
cp "$SCRIPT_DIR/Design/vRemoter-Logo-v1/vRemoter-app-icon-v9.png" \
  "$CONTENTS/Resources/vRemoterLogo.png"

ICONSET="$SCRIPT_DIR/dist/vRemoter.iconset"
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
sips -z 16 16 "$CONTENTS/Resources/vRemoterLogo.png" --out "$ICONSET/icon_16x16.png" >/dev/null
sips -z 32 32 "$CONTENTS/Resources/vRemoterLogo.png" --out "$ICONSET/icon_16x16@2x.png" >/dev/null
sips -z 32 32 "$CONTENTS/Resources/vRemoterLogo.png" --out "$ICONSET/icon_32x32.png" >/dev/null
sips -z 64 64 "$CONTENTS/Resources/vRemoterLogo.png" --out "$ICONSET/icon_32x32@2x.png" >/dev/null
sips -z 128 128 "$CONTENTS/Resources/vRemoterLogo.png" --out "$ICONSET/icon_128x128.png" >/dev/null
sips -z 256 256 "$CONTENTS/Resources/vRemoterLogo.png" --out "$ICONSET/icon_128x128@2x.png" >/dev/null
sips -z 256 256 "$CONTENTS/Resources/vRemoterLogo.png" --out "$ICONSET/icon_256x256.png" >/dev/null
sips -z 512 512 "$CONTENTS/Resources/vRemoterLogo.png" --out "$ICONSET/icon_256x256@2x.png" >/dev/null
sips -z 512 512 "$CONTENTS/Resources/vRemoterLogo.png" --out "$ICONSET/icon_512x512.png" >/dev/null
cp "$CONTENTS/Resources/vRemoterLogo.png" "$ICONSET/icon_512x512@2x.png"
iconutil -c icns "$ICONSET" -o "$CONTENTS/Resources/vRemoter.icns"
rm -rf "$ICONSET"

if [[ -d "$SCRIPT_DIR/Resources/PermissionGuides" ]]; then
  ditto "$SCRIPT_DIR/Resources/PermissionGuides" \
    "$CONTENTS/Resources/PermissionGuides"
fi
if [[ -d "$SCRIPT_DIR/Resources/buymeacoffee" ]]; then
  ditto "$SCRIPT_DIR/Resources/buymeacoffee" \
    "$CONTENTS/Resources/BuyMeACoffee"
fi
if [[ -d "$SCRIPT_DIR/Resources/Commerce" ]]; then
  ditto "$SCRIPT_DIR/Resources/Commerce" \
    "$CONTENTS/Resources/Commerce"
fi
if [[ -d "$SCRIPT_DIR/Resources/RemoteImages" ]]; then
  ditto "$SCRIPT_DIR/Resources/RemoteImages" \
    "$CONTENTS/Resources/RemoteImages"
fi
chmod 755 "$CONTENTS/MacOS/vRemote"
# SwiftPM resource bundles may contain read-only privacy manifests. The app
# bundle is a disposable build artifact, so make it owner-writable before
# clearing inherited metadata and applying the final ad-hoc signature.
chmod -R u+w "$APP"
xattr -cr "$APP"

# Give the ad-hoc build a stable designated requirement. Without this,
# codesign falls back to a CDHash-only requirement, so every rebuild looks
# like a different application to Accessibility/Input Monitoring (TCC).
codesign \
  --force \
  --deep \
  --sign - \
  --timestamp=none \
  --requirements '=designated => identifier "local.simaqingfeng.vRemote"' \
  "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"
print "Built: $APP"
