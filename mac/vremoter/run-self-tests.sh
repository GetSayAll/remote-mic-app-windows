#!/bin/zsh
set -euo pipefail

SCRIPT_DIR="${0:A:h}"
OUTPUT="$SCRIPT_DIR/.build/vremote-self-test"

mkdir -p "${OUTPUT:h}"
swiftc \
  "$SCRIPT_DIR/Sources/vRemote/ATVV/BridgeError.swift" \
  "$SCRIPT_DIR/Sources/vRemote/ATVV/ADPCMDecoder.swift" \
  "$SCRIPT_DIR/Sources/vRemote/ATVV/ATVVProtocol.swift" \
  "$SCRIPT_DIR/SelfTests/main.swift" \
  -o "$OUTPUT"
"$OUTPUT"
