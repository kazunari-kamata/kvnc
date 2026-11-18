#!/bin/sh
# Wake a sleeping display before desktop capture, then keep only system sleep inhibited.
set -eu

script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
kvnc_binary=${KVNC_BINARY:-"$script_directory/../bin/kvnc"}

# A standalone assertion is required before CoreGraphics starts desktop capture.
# Combining `-u -t` with the long-lived command does not reliably wake an
# already-sleeping macOS display before the child begins capture.
/usr/bin/caffeinate -u -t 5
exec /usr/bin/caffeinate -i "$kvnc_binary" "$@"
