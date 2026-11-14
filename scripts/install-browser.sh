#!/bin/sh
# Embedded in the CLI. Installs the tested fork privately, never upstream.
set -eu
umask 077
version=0.41.1
asset=agent-browser-browse-darwin-arm64.tar.gz
archive_sha=
native_sha=
platform="$(uname -s):$(uname -m)"
case "$platform" in
  Darwin:arm64) ;;
  Linux:x86_64|Linux:amd64)
    libc=$(getconf GNU_LIBC_VERSION 2>/dev/null) || libc=
    if ! printf '%s\n' "$libc" | awk '
      NF==2 && $1=="glibc" && $2~/^[0-9]+\.[0-9]+$/ {
        split($2, version, ".")
        supported=(version[1]>2 || (version[1]==2 && version[2]>=39))
      }
      END { exit !(NR==1 && supported) }
    '; then
      echo 'Slingring on Linux requires glibc 2.39 or newer (for example, Ubuntu 24.04).' >&2
      exit 1
    fi
    version=0.43.0
    asset=agent-browser-browse-linux-x86_64.tar.gz
    # The release assets can be replaced. Verify against these independent pins,
    # not only the checksum supplied beside the archive.
    archive_sha=4b4295176517a10b9682ef06ab1f4d4e740ac145d4bbf1af9e5255bf308d5281
    native_sha=1465dcc849415607e5bdcb40e08f26469ec5776881d0b19787a7b97898b74c8d
    upstream_commit=1cc19526f3641c8543d55aa6700e7c55b37ac356
    patch_sha=0e7adc3309844070a846fa6985880189007ba5fcb9ac230e5f3e5c50ed42111b
    ;;
  *) echo 'The bundled browser helper supports Apple Silicon macOS and x86_64 Linux only.' >&2; exit 1 ;;
esac
base_url="https://github.com/amxv/agent-browser-browse/releases/download/v$version"
root=${1:?Slingring home required}
case "$root" in /*) ;; *) echo 'Slingring home must be absolute' >&2; exit 1;; esac
base="$root/browser"
destination="$base/$version"
[ ! -L "$base" ] && [ ! -L "$destination" ] || { echo 'Refusing a symlink browser directory' >&2; exit 1; }
if [ -n "$archive_sha" ]; then
  for path in "$root" "$base/agent-browser-browse" "$base/config.json"; do
    [ ! -L "$path" ] || { echo 'Refusing a symlink Linux browser installation path' >&2; exit 1; }
  done
  for path in "$base/agent-browser-browse" "$base/config.json"; do
    [ ! -e "$path" ] || [ -f "$path" ] || { echo 'Refusing a nonregular Linux browser installation file' >&2; exit 1; }
  done
fi
mkdir -p "$base"
chmod 700 "$base"
stage=$(mktemp -d "$base/.install.XXXXXXXX")
trap 'rm -rf "$stage"' EXIT HUP INT TERM
linux_hash() {
  hash_line=$(sha256sum -- "$1") || return 1
  printf '%s\n' "${hash_line%% *}"
}
linux_download() {
  if command -v gh >/dev/null 2>&1 && GH_PROMPT_DISABLED=1 gh auth status >/dev/null 2>&1; then
    GH_PROMPT_DISABLED=1 gh release download "v$version" --repo amxv/agent-browser-browse --pattern "$1" --output "$2"
  else
    curl --proto '=https' --tlsv1.2 -fsSL --retry 2 --connect-timeout 15 --max-time 240 "$base_url/$1" -o "$2"
  fi
}
valid() {
  candidate=$1
  if [ -n "${native_sha:-}" ]; then
    for member in agent-browser-browse-native LICENSE.upstream UPSTREAM_VERSION UPSTREAM_COMMIT PATCH_SHA256; do
      [ -f "$candidate/$member" ] && [ ! -L "$candidate/$member" ] || return 1
    done
    [ "$(cat "$candidate/UPSTREAM_VERSION")" = "$version" ] &&
      [ "$(cat "$candidate/UPSTREAM_COMMIT")" = "$upstream_commit" ] &&
      [ "$(cat "$candidate/PATCH_SHA256")" = "$patch_sha" ] &&
      [ "$(linux_hash "$candidate/agent-browser-browse-native")" = "$native_sha" ] &&
      [ "$(od -An -t u1 -N 6 "$candidate/agent-browser-browse-native" | xargs)" = '127 69 76 70 2 1' ] &&
      [ "$(od -An -t u1 -j 18 -N 2 "$candidate/agent-browser-browse-native" | xargs)" = '62 0' ] &&
      [ "$("$candidate/agent-browser-browse-native" --version)" = "agent-browser $version" ]
    return $?
  fi
  [ -f "$candidate/agent-browser-browse-native" ] && [ ! -L "$candidate/agent-browser-browse-native" ] &&
    [ "$(cat "$candidate/UPSTREAM_VERSION" 2>/dev/null)" = "$version" ] &&
    codesign --verify --strict "$candidate/agent-browser-browse-native" >/dev/null 2>&1 &&
    [ "$(lipo -archs "$candidate/agent-browser-browse-native")" = arm64 ] &&
    [ "$("$candidate/agent-browser-browse-native" --version)" = "agent-browser $version" ]
}
if ! valid "$destination"; then
  echo "Installing the verified Slingring browser helper ($version)…" >&2
  if [ -n "${archive_sha:-}" ]; then
    [ ! -e "$destination" ] || {
      echo 'Existing managed browser failed verification; refusing to overwrite it.' >&2; exit 1
    }
    linux_download "$asset" "$stage/$asset"
    linux_download SHA256SUMS "$stage/SHA256SUMS"
    published=$(awk -v name="$asset" '$2 == name {n++; value=$1} END {if(n==1) print value}' "$stage/SHA256SUMS")
    [ "$published" = "$archive_sha" ] && [ "$(linux_hash "$stage/$asset")" = "$archive_sha" ] || {
      echo 'Linux browser checksum does not match the reviewed release pin.' >&2; exit 1
    }
    tar --quoting-style=escape -tzf "$stage/$asset" > "$stage/members"
    printf '%s\n' LICENSE.upstream PATCH_SHA256 UPSTREAM_COMMIT UPSTREAM_VERSION agent-browser-browse-native > "$stage/expected"
    LC_ALL=C sort "$stage/members" > "$stage/sorted"
    cmp -s "$stage/expected" "$stage/sorted" || { echo 'Unexpected Linux browser archive members' >&2; exit 1; }
    tar --quoting-style=escape -tvzf "$stage/$asset" > "$stage/types"
    awk 'substr($0,1,1)!="-" {bad=1} END {exit (bad || NR != 5)}' "$stage/types" || {
      echo 'Unsafe Linux browser archive entry' >&2; exit 1
    }
    mkdir "$stage/unpacked"
    for member in agent-browser-browse-native LICENSE.upstream UPSTREAM_VERSION UPSTREAM_COMMIT PATCH_SHA256; do
      tar -xOzf "$stage/$asset" -- "$member" > "$stage/unpacked/$member"
    done
  else
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
  fi
  chmod 755 "$stage/unpacked/agent-browser-browse-native"
  valid "$stage/unpacked" || { echo 'Browser architecture, signature, or version verification failed' >&2; exit 1; }
  if [ -e "$destination" ]; then
    echo 'Existing managed browser failed verification; refusing to overwrite it. Move the browser version directory aside and retry.' >&2; exit 1
  fi
  mv "$stage/unpacked" "$destination"
fi
printf '{}\n' > "$stage/config.json"
mv "$stage/config.json" "$base/config.json"
if [ -n "${native_sha:-}" ]; then
cat > "$stage/agent-browser-browse" <<'LINUX_WRAPPER'
#!/bin/sh
set -eu
umask 077
base=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export AGENT_BROWSER_NAMESPACE=slingring
export AGENT_BROWSER_CONFIG="$base/config.json"
exec "$base/0.43.0/agent-browser-browse-native" "$@"
LINUX_WRAPPER
else
cat > "$stage/agent-browser-browse" <<'WRAPPER'
#!/bin/sh
set -eu
base=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
export AGENT_BROWSER_NAMESPACE=slingring
export AGENT_BROWSER_CONFIG="$base/config.json"
exec "$base/0.41.1/agent-browser-browse-native" "$@"
WRAPPER
fi
chmod 755 "$stage/agent-browser-browse"
[ ! -L "$base/agent-browser-browse" ] || { echo 'Refusing a symlink browser wrapper' >&2; exit 1; }
mv "$stage/agent-browser-browse" "$base/agent-browser-browse"
echo 'Browser helper ready. No separate installation is needed.' >&2
