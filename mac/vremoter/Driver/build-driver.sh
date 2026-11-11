#!/bin/zsh
set -euo pipefail

SOURCE_DRIVER="/Library/Audio/Plug-Ins/HAL/BlackHole2ch.driver"
SCRIPT_DIR="${0:A:h}"
BUILD_DIR="$SCRIPT_DIR/build"
OUTPUT_DRIVER="$BUILD_DIR/vRemoteDriver.driver"
EXECUTABLE="$OUTPUT_DRIVER/Contents/MacOS/BlackHole"
PLIST="$OUTPUT_DRIVER/Contents/Info.plist"

if [[ ! -d "$SOURCE_DRIVER" ]]; then
  print -u2 "Missing source driver: $SOURCE_DRIVER"
  exit 1
fi

rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR"
ditto "$SOURCE_DRIVER" "$OUTPUT_DRIVER"
rm -rf "$OUTPUT_DRIVER/Contents/_CodeSignature"

# Keep every binary replacement exactly the same byte length. This lets the
# proof of concept reuse the installed universal BlackHole binary without Xcode.
perl -0777 -pi -e '
  my $x86TransportSeen = 0;
  my $armTransportSeen = 0;
  s/audio\.existential\.BlackHole2ch/audio.local.vRemoteDriver2chXX/g;
  s/BlackHole%ich_UID/vRemoteDr%ich_UID/g;
  s/BlackHole%ich_2_UID/vRemoteDr%ich_2_UID/g;
  s/BlackHole%ich_ModelUID/vRemoteDr%ich_ModelUID/g;
  s/BlackHole Box/vRemoteDr Box/g;
  s/BlackHole %ich/vRemoteDr %ich/g;
  s/triv/(++$x86TransportSeen == 2) ? " bsu" : "triv"/ge;
  s/(\x88\x4e\x8e\x52\x28\xcd\xae\x72)/
    (++$armTransportSeen == 2)
      ? "\x08\x44\x8c\x52\x68\xae\xae\x72"
      : $1
  /gex;
' "$EXECUTABLE"

/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier audio.local.vRemoteDriver2chXX" "$PLIST"
/usr/libexec/PlistBuddy -c "Set :CFBundleName vRemote Driver" "$PLIST"
/usr/libexec/PlistBuddy -c "Delete :CFPlugInFactories:9CAC157F-784A-4552-A1DE-0B9840FC974C" "$PLIST"
/usr/libexec/PlistBuddy -c "Add :CFPlugInFactories:B7C4C615-9E72-4C83-A329-71C4E50E4A91 string BlackHole_Create" "$PLIST"
/usr/libexec/PlistBuddy -c "Set :CFPlugInTypes:443ABAB8-E7B3-491A-B985-BEB9187030DB:0 B7C4C615-9E72-4C83-A329-71C4E50E4A91" "$PLIST"

codesign --force --deep --sign - --timestamp=none "$OUTPUT_DRIVER"

print "Built: $OUTPUT_DRIVER"
print "Bundle ID: $(defaults read "$PLIST" CFBundleIdentifier)"
print "Architectures: $(lipo -archs "$EXECUTABLE")"
codesign --verify --deep --strict --verbose=2 "$OUTPUT_DRIVER"
