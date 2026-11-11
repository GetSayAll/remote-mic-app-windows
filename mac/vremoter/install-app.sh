#!/bin/zsh
set -euo pipefail

SCRIPT_DIR="${0:A:h}"
SOURCE="$SCRIPT_DIR/dist/build/vRemote.app"
DESTINATION="$HOME/Applications/vRemote.app"

if [[ ! -d "$SOURCE" ]]; then
  print -u2 "Run package-app.sh first."
  exit 1
fi

mkdir -p "$HOME/Applications"
rm -rf "$DESTINATION"
ditto "$SOURCE" "$DESTINATION"
codesign --verify --deep --strict --verbose=2 "$DESTINATION"
print "Installed: $DESTINATION"
