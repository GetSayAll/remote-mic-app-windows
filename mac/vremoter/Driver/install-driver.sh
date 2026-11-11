#!/bin/zsh
set -euo pipefail

SCRIPT_DIR="${0:A:h}"
SOURCE="$SCRIPT_DIR/build/vRemoteDriver.driver"
DESTINATION="/Library/Audio/Plug-Ins/HAL/vRemoteDriver.driver"

if [[ ! -d "$SOURCE" ]]; then
  print -u2 "Build the proof-of-concept driver first: $SOURCE"
  exit 1
fi

if [[ -e "$DESTINATION" ]]; then
  print -u2 "Refusing to overwrite existing path: $DESTINATION"
  exit 2
fi

ditto "$SOURCE" "$DESTINATION"
chown -R root:wheel "$DESTINATION"
find "$DESTINATION" -type d -exec chmod 755 {} \;
find "$DESTINATION" -type f -exec chmod 644 {} \;
chmod 755 "$DESTINATION/Contents/MacOS/BlackHole"

killall -9 coreaudiod 2>/dev/null || true
print "Installed: $DESTINATION"
