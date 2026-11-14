#!/bin/sh
# Embedded in the CLI. Installs the tested fork privately, never upstream.
set -eu
umask 077
version=0.41.1
asset=agent-browser-browse-darwin-arm64.tar.gz
base_url="https://github.com/amxv/agent-browser-browse/releases/download/v$version"
root=${1:?Slingring home required}
case "$root" in /*) ;; *) echo 'Slingring home must be absolute' >&2; exit 1;; esac
[ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] || {
  echo 'The bundled browser helper currently supports Apple Silicon Macs only.' >&2; exit 1;
}
base="$root/browser"
destination="$base/$version"
[ ! -L "$base" ] && [ ! -L "$destination" ] || { echo 'Refusing a symlink browser directory' >&2; exit 1; }
mkdir -p "$base"
chmod 700 "$base"
stage=$(mktemp -d "$base/.install.XXXXXXXX")
trap 'rm -rf "$stage"' EXIT HUP INT TERM
valid() {
  candidate=$1
  [ -f "$candidate/agent-browser-browse-native" ] && [ ! -L "$candidate/agent-browser-browse-native" ] &&
    [ "$(cat "$candidate/UPSTREAM_VERSION" 2>/dev/null)" = "$version" ] &&
    codesign --verify --strict "$candidate/agent-browser-browse-native" >/dev/null 2>&1 &&
    [ "$(lipo -archs "$candidate/agent-browser-browse-native")" = arm64 ] &&
    [ "$("$candidate/agent-browser-browse-native" --version)" = "agent-browser $version" ]
}
if ! valid "$destination"; then
  echo "Installing the verified Slingring browser helper ($version)…" >&2
  curl --proto '=https' --tlsv1.2 -fsSL --retry 2 --connect-timeout 15 --max-time 240 "$base_url/$asset" -o "$stage/$asset"
  curl --proto '=https' --tlsv1.2 -fsSL --retry 2 --connect-timeout 15 --max-time 60 "$base_url/SHA256SUMS" -o "$stage/SHA256SUMS"
  expected=$(awk -v name="$asset" '$2 == name {n++; value=$1} END {if(n==1) print value}' "$stage/SHA256SUMS")
  printf '%s' "$expected" | grep -Eq '^[[:xdigit:]]{64}$' || { echo 'Missing or ambiguous browser checksum' >&2; exit 1; }
  actual=$(shasum -a 256 "$stage/$asset" | awk '{print $1}')
  [ "$actual" = "$expected" ] || { echo 'Browser checksum mismatch; nothing replaced' >&2; exit 1; }
  tar -tzf "$stage/$asset" > "$stage/members"
  while IFS= read -r name; do
    case "$name" in .|./) continue;; esac
    name=${name#./}
    case "$name" in agent-browser-browse-native|LICENSE.upstream|UPSTREAM_VERSION|UPSTREAM_COMMIT|PATCH_SHA256) ;; *) echo 'Unexpected browser archive member' >&2; exit 1;; esac
  done < "$stage/members"
  # Only regular files and the archive's root directory are accepted.
  tar -tvzf "$stage/$asset" | awk 'substr($0,1,1)!="-" && substr($0,1,1)!="d" {bad=1} END {exit bad}' || {
    echo 'Unsafe browser archive entry' >&2; exit 1;
  }
  mkdir "$stage/unpacked"
  tar -xzf "$stage/$asset" -C "$stage/unpacked"
  chmod 755 "$stage/unpacked/agent-browser-browse-native"
  valid "$stage/unpacked" || { echo 'Browser architecture, signature, or version verification failed' >&2; exit 1; }
  if [ -e "$destination" ]; then
    echo 'Existing managed browser failed verification; refusing to overwrite it. Move the browser version directory aside and retry.' >&2; exit 1
  fi
  mv "$stage/unpacked" "$destination"
fi
printf '{}\n' > "$stage/config.json"
mv "$stage/config.json" "$base/config.json"
cat > "$stage/agent-browser-browse" <<'WRAPPER'
#!/bin/sh
set -eu
base=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export AGENT_BROWSER_NAMESPACE=slingring
export AGENT_BROWSER_CONFIG="$base/config.json"
exec "$base/0.41.1/agent-browser-browse-native" "$@"
WRAPPER
chmod 755 "$stage/agent-browser-browse"
[ ! -L "$base/agent-browser-browse" ] || { echo 'Refusing a symlink browser wrapper' >&2; exit 1; }
mv "$stage/agent-browser-browse" "$base/agent-browser-browse"
echo 'Browser helper ready. No separate installation is needed.' >&2
