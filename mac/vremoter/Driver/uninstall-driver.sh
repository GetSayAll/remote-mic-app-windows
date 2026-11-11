#!/bin/zsh
set -euo pipefail

DESTINATION="/Library/Audio/Plug-Ins/HAL/vRemoteDriver.driver"

rm -rf "$DESTINATION"
killall -9 coreaudiod 2>/dev/null || true
print "Removed: $DESTINATION"
