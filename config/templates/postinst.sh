#!/bin/sh
# {{product.name}} package hook, run after the .deb or .rpm package is installed or upgraded.
#
# Stops every user's copy whose program was just replaced: it holds the app's endpoint and may
# refuse the new version's hand-over (D221). Only the new package's hook reaches earlier versions.

REPLACED="/usr/bin/{{executables.app}} (deleted)"

for exe in /proc/[0-9]*/exe; do
  [ "$(readlink "$exe" 2>/dev/null)" = "$REPLACED" ] || continue
  pid=${exe#/proc/}
  kill "${pid%/exe}" 2>/dev/null || true
done

exit 0
