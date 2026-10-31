#!/usr/bin/env bash
# Installs the Ubuntu packages a list names, for CI (D55).
#
# Usage: bash scripts/ci/linux-packages.sh LIST CACHE_DIR
#
# LIST names one package per line (`#` starts a comment). With the .deb files of an earlier run in CACHE_DIR, which CI restores for the same runner image and list, they are installed with dpkg and nothing is downloaded. Otherwise they are downloaded and left in CACHE_DIR for the cache to keep.
#
# A mirror sometimes slows to a crawl for one runner, once for 49 minutes, so no download may take long: every mirror gets a probe of PROBE_SECONDS, and the mirrors are tried fastest first, each with STEP_SECONDS for its package lists and again for the packages. A mirror that runs out of time, or leaves a file with no data for HANG_SECONDS, gives way to the next; what it already downloaded is kept. apt reads only the source list written here, so no other repository on the runner can hold it up either. Every mirror serves the same files signed by Ubuntu's archive key, which apt checks.
set -euo pipefail

arch=$(dpkg --print-architecture)
if [ "$arch" = amd64 ]; then
  MIRRORS=(
    http://azure.archive.ubuntu.com/ubuntu
    http://archive.ubuntu.com/ubuntu
    http://us.archive.ubuntu.com/ubuntu
    http://mirrors.edge.kernel.org/ubuntu
  )
else
  # Every other architecture is served from Ubuntu's ports archive.
  MIRRORS=(
    http://azure.ports.ubuntu.com/ubuntu-ports
    http://ports.ubuntu.com/ubuntu-ports
  )
fi
# The first PROBE_BYTES of a large file every mirror has; a mirror that cannot send them within PROBE_SECONDS is slow.
PROBE_BYTES=$((10 * 1024 * 1024))
PROBE_SECONDS=10
STEP_SECONDS=120
HANG_SECONDS=15

list=$1
cache=$2
sudo=()
[ "$(id -u)" = 0 ] || sudo=(sudo)

mapfile -t packages < <(sed -e 's/#.*//' -e 's/[[:space:]]//g' "$list" | grep -v '^$')
mkdir -p "$cache"

if compgen -G "$cache/*.deb" >/dev/null; then
  echo "Installing ${#packages[@]} packages from the cache."
  if "${sudo[@]}" dpkg --install "$cache"/*.deb; then
    exit 0
  fi
  echo "::warning::The cached packages did not install; downloading them instead."
  rm -f "$cache"/*.deb
fi

codename=$(. /etc/os-release && echo "$VERSION_CODENAME")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

echo "Probing ${#MIRRORS[@]} mirrors for ${PROBE_SECONDS} seconds."
for i in "${!MIRRORS[@]}"; do
  curl --silent --output /dev/null --range "0-$((PROBE_BYTES - 1))" --max-time "$PROBE_SECONDS" \
    --write-out '%{speed_download}' "${MIRRORS[$i]}/dists/$codename/Contents-$arch.gz" >"$work/probe-$i" 2>/dev/null &
done
wait
ranked=()
while read -r speed mirror; do
  awk -v s="$speed" -v m="$mirror" 'BEGIN { printf "  %6.1f MB/s  %s\n", s / 1048576, m }'
  ranked+=("$mirror")
done < <(for i in "${!MIRRORS[@]}"; do
  speed=$(cat "$work/probe-$i")
  echo "${speed:-0} ${MIRRORS[$i]}"
done | sort -rn -k1,1)

apt=(apt-get -q -y
  -o "Dir::Etc::SourceList=/dev/null" -o "Dir::Etc::SourceParts=$work/sources"
  -o "Acquire::http::Timeout=$HANG_SECONDS" -o "Acquire::Retries=1"
  -o "APT::Keep-Downloaded-Packages=true")
mkdir -p "$work/sources"
# So the cache gets this install's packages and nothing else.
"${sudo[@]}" rm -f /var/cache/apt/archives/*.deb

for mirror in "${ranked[@]}"; do
  echo "Downloading from $mirror."
  cat >"$work/sources/ubuntu.sources" <<EOF
Types: deb
URIs: $mirror/
Suites: $codename $codename-updates $codename-security
Components: main restricted universe multiverse
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg
EOF
  if timeout --kill-after=10 "$STEP_SECONDS" "${sudo[@]}" "${apt[@]}" update &&
    timeout --kill-after=10 "$STEP_SECONDS" "${sudo[@]}" "${apt[@]}" install --download-only --fix-broken --no-install-recommends "${packages[@]}"; then
    cp /var/cache/apt/archives/*.deb "$cache"/ 2>/dev/null || true
    "${sudo[@]}" "${apt[@]}" install --no-download --fix-broken --no-install-recommends "${packages[@]}"
    exit 0
  fi
  echo "::warning::$mirror did not finish within ${STEP_SECONDS} seconds; trying the next mirror."
done

echo "::error::No mirror delivered the packages."
exit 1
