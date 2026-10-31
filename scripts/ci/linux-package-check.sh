#!/usr/bin/env bash
# Installs a Linux package on a fresh system and checks it works, for the release workflow (D106).
#
# Usage: bash scripts/ci/linux-package-check.sh deb|rpm|appimage FILE APP
#
# Runs as root, in a container of the system the format is for (Ubuntu for deb, Fedora for rpm), or on the runner for an AppImage. The package's dependencies must cover every library the app loads; the app must start under a virtual display and stay up, registering the browser bridge for its user; and removing a deb or rpm must take that registration away again.
set -euo pipefail

KIND=$1
FILE=$(realpath "$2")
APP=$3
USER_NAME=wirecatch-check
# Long enough to start, register the bridge and draw; the app runs until stopped.
RUN_SECONDS=15

case "$KIND" in
  deb)
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -q
    apt-get install -y -q "$FILE" xvfb xauth mesa-vulkan-drivers >/dev/null
    ;;
  rpm)
    dnf install -y -q "$FILE" xorg-x11-server-Xvfb xorg-x11-xauth mesa-vulkan-drivers util-linux which >/dev/null
    ;;
  appimage)
    chmod +x "$FILE"
    ;;
  *)
    echo "unknown package kind: $KIND" >&2
    exit 2
    ;;
esac

id "$USER_NAME" >/dev/null 2>&1 || useradd -m -u 1500 "$USER_NAME"
HOME_DIR=$(getent passwd "$USER_NAME" | cut -d: -f6)

if [ "$KIND" = appimage ]; then
  cp "$FILE" "$HOME_DIR/app.AppImage"
  chown "$USER_NAME" "$HOME_DIR/app.AppImage"
  COMMAND=("$HOME_DIR/app.AppImage")
else
  BINARY=$(command -v "$APP")
  missing=$(ldd "$BINARY" | grep 'not found' || true)
  if [ -n "$missing" ]; then
    echo "The package leaves libraries out:" >&2
    echo "$missing" >&2
    exit 1
  fi
  COMMAND=("$BINARY")
fi

status=0
# No FUSE in containers and runners: an AppImage unpacks itself instead of mounting.
timeout "$RUN_SECONDS" runuser -u "$USER_NAME" -- env HOME="$HOME_DIR" APPIMAGE_EXTRACT_AND_RUN=1 \
  xvfb-run -a "${COMMAND[@]}" || status=$?
if [ "$status" -ne 124 ]; then
  echo "The app stopped by itself (exit $status) instead of running until stopped." >&2
  exit 1
fi

registered() {
  find "$HOME_DIR" -path '*NativeMessagingHosts/*.json' -o -path '*native-messaging-hosts/*.json' | grep -c . || true
}
if [ "$(registered)" -eq 0 ]; then
  echo "The app did not register the browser bridge." >&2
  exit 1
fi
echo "The app ran and registered the bridge in $(registered) browser folders."

case "$KIND" in
  deb) apt-get remove -y -q "$(dpkg-deb -f "$FILE" Package)" >/dev/null ;;
  rpm) dnf remove -y -q "$(rpm -qp --qf '%{NAME}' "$FILE")" >/dev/null ;;
  appimage) exit 0 ;;
esac
if [ "$(registered)" -ne 0 ]; then
  echo "Removing the package left the bridge registered." >&2
  exit 1
fi
echo "Removing the package took the registration away."
