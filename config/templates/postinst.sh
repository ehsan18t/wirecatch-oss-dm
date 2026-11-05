#!/bin/sh
# {{product.name}} package hook, run after the .deb or .rpm package is installed or upgraded.
#
# Stops every user's copy whose program was just replaced: it holds the app's endpoint and may
# refuse the new version's hand-over (D221). Only the new package's hook reaches earlier versions.

REPLACED="/usr/bin/{{executables.app}} (deleted)"
PATIENCE=10

replaced_copies() {
  for exe in /proc/[0-9]*/exe; do
    [ "$(readlink "$exe" 2>/dev/null)" = "$REPLACED" ] || continue
    pid=${exe#/proc/}
    echo "${pid%/exe}"
  done
}

copies=$(replaced_copies)
[ -n "$copies" ] || exit 0
kill $copies 2>/dev/null

waited=0
while [ -n "$(replaced_copies)" ] && [ "$waited" -lt "$PATIENCE" ]; do
  sleep 1
  waited=$((waited + 1))
done

copies=$(replaced_copies)
[ -z "$copies" ] || kill -9 $copies 2>/dev/null

exit 0
