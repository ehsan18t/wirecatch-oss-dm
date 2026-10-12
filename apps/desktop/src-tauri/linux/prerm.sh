#!/bin/sh
# Generated from config/templates/prerm.sh and config/product.toml by `pnpm product:generate`. Do not edit: change those and run it again.
# Wirecatch package hook, run before the .deb or .rpm package is removed.
#
# The app registers the browser bridge for each user on every start (D19), in that user's
# browser folders. On removal this undoes it for every regular user, so no browser is left
# pointing at a bridge that no longer exists. On an upgrade it does nothing: the new version
# registers itself again on its next start.

case "$1" in
  remove | purge | 0) ;; # deb: remove or purge; rpm: 0 versions left after an erase
  *) exit 0 ;;
esac

BRIDGE=/usr/bin/wirecatch-bridge
[ -x "$BRIDGE" ] || exit 0

getent passwd | while IFS=: read -r user _ uid _ _ home _; do
  [ "$uid" -ge 1000 ] 2>/dev/null || continue
  [ "$uid" -lt 60000 ] || continue
  [ -d "$home" ] || continue
  runuser -u "$user" -- env HOME="$home" "$BRIDGE" uninstall >/dev/null 2>&1 || true
done

exit 0
