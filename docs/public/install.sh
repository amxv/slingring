#!/bin/sh
# Installs the macOS CLI and optional signed Swift app. Private releases use authenticated gh.
# Public releases are ad-hoc signed. An explicit local identity can preserve
# macOS permissions across upgrades; never change Keychain trust or quarantine.
set -eu
cli='slingring'
binary='slingring'
app='Slingring.app'
app_exec='slingring-menubar'
repo='https://github.com/amxv/slingring'
action=${1:-install}
case "$action" in install|upgrade|--check) ;; *) echo "Usage: install.sh [upgrade|--check]" >&2; exit 2 ;; esac
case "$(uname -s)" in
  Darwin) os=darwin; mac=1 ;;
  *) echo 'Slingring currently supports macOS only.' >&2; exit 1 ;;
esac
case "$(uname -m)" in
  arm64|aarch64) arch=arm64 ;;
  x86_64|amd64) echo 'Slingring supports Apple Silicon Macs only.' >&2; exit 1 ;;
  *) echo 'Unsupported CPU architecture' >&2; exit 1 ;;
esac
install_dir=${CLI_INSTALL_DIR:-"$HOME/.local/bin"}
case "$install_dir" in /*) ;; *) echo 'CLI_INSTALL_DIR must be absolute' >&2; exit 1 ;; esac
target=${CLI_UPGRADE_TARGET:-"$install_dir/$cli"}
case "$target" in /*) ;; *) echo 'CLI_UPGRADE_TARGET must be absolute' >&2; exit 1 ;; esac
install_dir=$(dirname "$target")
app_dest="$install_dir/$app"
if [ -L "$target" ] || [ -L "$app_dest" ]; then
  echo 'Refusing to replace a symlink installation' >&2; exit 1
fi
if [ "$action" = install ] && { [ -e "$target" ] || [ -e "$app_dest" ]; }; then
  echo "$cli is installed. Use '$cli upgrade' rather than reinstalling." >&2; exit 1
fi
if [ "$action" = upgrade ] && [ ! -f "$target" ]; then
  echo "No installed CLI at $target" >&2; exit 1
fi
if [ "$action" = --check ]; then
  printf 'Installed: '
  if [ -x "$target" ]; then "$target" --version; else echo '(not installed)'; fi
  if command -v gh >/dev/null 2>&1 && GH_PROMPT_DISABLED=1 gh auth status >/dev/null 2>&1; then
    gh release view --repo amxv/slingring --json tagName --jq '.tagName'
  else
    curl --proto '=https' --tlsv1.2 -fsSIL --max-time 20 -o /dev/null \
      -w 'Latest release: %{url_effective}\n' "$repo/releases/latest"
  fi
  exit 0
fi

version=${CLI_VERSION:-}
if [ -z "$version" ] && command -v gh >/dev/null 2>&1; then
  version=$(gh api repos/amxv/slingring/releases/latest --jq .tag_name 2>/dev/null || true)
fi
if [ -n "$version" ]; then
  version=${version#v}
  printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || { echo 'Invalid CLI_VERSION' >&2; exit 1; }
  base="$repo/releases/download/v$version"
else
  base="$repo/releases/latest/download"
fi
for tool in curl awk tar; do
  command -v "$tool" >/dev/null 2>&1 || { echo "$tool is required" >&2; exit 1; }
done
umask 077
stage=$(mktemp -d "${TMPDIR:-/tmp}/$cli-install.XXXXXXXX")
trap 'rm -rf "$stage"' EXIT HUP INT TERM
download_asset() {
  name=$1
  destination=$2
  if command -v gh >/dev/null 2>&1 && GH_PROMPT_DISABLED=1 gh auth status >/dev/null 2>&1; then
    if [ -n "$version" ]; then
      GH_PROMPT_DISABLED=1 gh release download "v$version" --repo amxv/slingring --pattern "$name" --output "$destination"
    else
      GH_PROMPT_DISABLED=1 gh release download --repo amxv/slingring --pattern "$name" --output "$destination"
    fi
  else
    curl --proto '=https' --tlsv1.2 -fsSL --retry 3 "$base/$name" -o "$destination" || {
      echo 'Release download failed. Check network and release availability; private releases also require an authenticated gh CLI with repository access.' >&2
      return 1
    }
  fi
}
asset="${binary}_darwin_${arch}.tar.gz"
download_asset "$asset.sha256" "$stage/sums"
download_asset "$asset" "$stage/$asset"
expected=$(awk 'NF { n++; value=$1 } END { if (n == 1) print value }' "$stage/sums")
printf '%s' "$expected" | grep -Eq '^[[:xdigit:]]{64}$' || { echo 'Missing or ambiguous release checksum' >&2; exit 1; }
if command -v shasum >/dev/null 2>&1; then
  actual=$(shasum -a 256 "$stage/$asset" | awk '{print $1}')
else
  actual=$(sha256sum "$stage/$asset" | awk '{print $1}')
fi
[ "$actual" = "$expected" ] || { echo 'Checksum mismatch; installation unchanged' >&2; exit 1; }

if [ "$mac" = 1 ]; then
  mkdir "$stage/unpacked"
  tar -tzf "$stage/$asset" | while IFS= read -r entry; do
    case "$entry" in /*|../*|*/../*|*/..) echo 'Unsafe archive path' >&2; exit 1 ;; esac
    case "$entry" in
      "$cli"|"$app"|"$app/"|"$app/"*|LICENSE) ;;
      *) echo "Unexpected or unsafe archive member: $entry" >&2; exit 1 ;;
    esac
  done
  tar -xzf "$stage/$asset" -C "$stage/unpacked"
  [ -z "$(find "$stage/unpacked" -type l -print -quit)" ] || { echo 'Archive symlinks are not allowed' >&2; exit 1; }
  [ -x "$stage/unpacked/$cli" ] || { echo 'Archive is missing the CLI' >&2; exit 1; }
  [ -x "$stage/unpacked/$app/Contents/MacOS/$app_exec" ] || { echo 'Archive is missing the menu bar app' >&2; exit 1; }
  codesign --verify --strict "$stage/unpacked/$cli"
  codesign --verify --deep --strict "$stage/unpacked/$app"
  "$stage/unpacked/$app/Contents/MacOS/$app_exec" --check
  source_cli="$stage/unpacked/$cli"
else
  source_cli="$stage/$asset"
  chmod 755 "$source_cli"
fi
# A certificate-backed local signature must survive an upgrade. Never silently
# replace it with the release's ad-hoc identity or rotate to another certificate.
# CLI_CODESIGN_IDENTITY is an explicit, unambiguous certificate SHA-1 fingerprint.
# No certificate, private key, trust setting, or Keychain ACL is created here.
signing_identity=${CLI_CODESIGN_IDENTITY:-}
if [ -n "$signing_identity" ]; then
  printf '%s' "$signing_identity" | grep -Eq '^[[:xdigit:]]{40}$' || {
    echo 'CLI_CODESIGN_IDENTITY must be the 40-digit SHA-1 fingerprint of a local code-signing certificate.' >&2; exit 1;
  }
  signing_identity=$(printf '%s' "$signing_identity" | tr '[:lower:]' '[:upper:]')
fi
installed_identity() {
  old_path=$1
  old_prefix=$2
  [ -e "$old_path" ] || return 0
  codesign --verify --deep --strict "$old_path" || {
    echo 'Installed signature could not be verified; installation unchanged.' >&2; return 1;
  }
  codesign --display --verbose=2 "$old_path" > "$old_prefix-info" 2>&1 || return 1
  sed -n 's/^Identifier=//p' "$old_prefix-info" > "$old_prefix-identifier"
  [ "$(wc -l < "$old_prefix-identifier" | tr -d ' ')" = 1 ] && [ -s "$old_prefix-identifier" ] || {
    echo 'Installed signing identifier is missing or ambiguous; installation unchanged.' >&2; return 1;
  }
  if grep -q '^Signature=adhoc$' "$old_prefix-info"; then return 0; fi
  command -v openssl >/dev/null 2>&1 || { echo 'openssl is required to preserve the installed signing identity.' >&2; return 1; }
  # Extract only the public certificate embedded in the installed executable.
  codesign --display --extract-certificates "$old_prefix-cert" "$old_path" >/dev/null 2>&1 || return 1
  [ -f "$old_prefix-cert0" ] || { echo 'Installed signing certificate is missing; installation unchanged.' >&2; return 1; }
  old_fingerprint=$(openssl x509 -inform DER -in "$old_prefix-cert0" -noout -sha1 -fingerprint | sed 's/.*=//;s/://g' | tr '[:lower:]' '[:upper:]')
  printf '%s' "$old_fingerprint" | grep -Eq '^[[:xdigit:]]{40}$' || return 1
  codesign --display --requirements - "$old_path" > "$old_prefix-requirements-output" 2>&1 || return 1
  sed -n 's/^# designated => /designated => /; /^designated => /p' "$old_prefix-requirements-output" > "$old_prefix-requirement"
  [ "$(wc -l < "$old_prefix-requirement" | tr -d ' ')" = 1 ] || {
    echo 'Installed designated requirement is missing or ambiguous; installation unchanged.' >&2; return 1;
  }
  printf '%s\n' "$old_fingerprint"
}
old_cli_identity=$(installed_identity "$target" "$stage/old-cli") || exit 1
old_app_identity=$(installed_identity "$app_dest" "$stage/old-app") || exit 1
for old_identity in "$old_cli_identity" "$old_app_identity"; do
  [ -n "$old_identity" ] || continue
  if [ -n "$signing_identity" ] && [ "$signing_identity" != "$old_identity" ]; then
    echo 'Refusing to change an installed signing certificate; installation unchanged.' >&2; exit 1
  fi
  signing_identity=$old_identity
done
if [ -n "$signing_identity" ]; then
  command -v security >/dev/null 2>&1 || { echo 'macOS security is required for local code signing.' >&2; exit 1; }
  security find-identity -p codesigning > "$stage/signing-identities"
  awk -v identity="$signing_identity" '$2 == identity { found=1 } END { exit !found }' "$stage/signing-identities" || {
    echo 'The installed or requested signing identity is unavailable in Keychain; installation unchanged. Restore that identity before upgrading.' >&2; exit 1;
  }
fi
mkdir -p "$install_dir"

# Stage BOTH replacements before stopping the menu bar process. If any move
# fails, recover the old CLI/app from the sibling backup directory.
staged=$(mktemp -d "$install_dir/.$cli-stage.XXXXXXXX")
backup=$(mktemp -d "$install_dir/.$cli-backup.XXXXXXXX")
trap 'rm -rf "$stage" "$staged" "$backup"' EXIT HUP INT TERM
cp "$source_cli" "$staged/$cli"
chmod 755 "$staged/$cli"
cp -R "$stage/unpacked/$app" "$staged/$app"
sign_staged() {
  sign_path=$1
  old_prefix=$2
  sign_identifier=$3
  if [ -s "$old_prefix-identifier" ]; then sign_identifier=$(cat "$old_prefix-identifier"); fi
  set -- --force --sign "$signing_identity" --timestamp=none --identifier "$sign_identifier" \
    --preserve-metadata=entitlements,flags,runtime
  if [ -s "$old_prefix-requirement" ]; then set -- "$@" --requirements "$old_prefix-requirement"; fi
  codesign "$@" "$sign_path" || {
    echo 'Local code signing failed; the working installation is unchanged. Check access to the selected signing key.' >&2; return 1;
  }
  if [ -s "$old_prefix-requirement" ]; then
    codesign --display --requirements - "$sign_path" > "$old_prefix-new-requirements-output" 2>&1 || return 1
    sed -n 's/^# designated => /designated => /; /^designated => /p' "$old_prefix-new-requirements-output" > "$old_prefix-new-requirement"
    cmp -s "$old_prefix-requirement" "$old_prefix-new-requirement" || {
      echo 'The designated requirement changed; installation unchanged.' >&2; return 1;
    }
    requirement=$(sed 's/^designated => //' "$old_prefix-requirement")
    codesign --verify --strict --test-requirement "=$requirement" "$sign_path" || return 1
  fi
}
if [ -n "$signing_identity" ]; then
  sign_staged "$staged/$cli" "$stage/old-cli" com.amxv.slingring.cli
  sign_staged "$staged/$app" "$stage/old-app" com.amxv.slingring.menubar
fi
codesign --verify --strict "$staged/$cli"
codesign --verify --deep --strict "$staged/$app"
"$staged/$cli" --version
# Install the verified browser dependency BEFORE touching the working CLI/app.
# Signing failures must not stop the running service or replace either binary.
"$staged/$cli" browser install >/dev/null
app_program="$app_dest/Contents/MacOS/$app_exec"
app_pids() {
  /bin/ps -axww -o uid= -o pid= -o command= 2>/dev/null |
    /usr/bin/awk -v uid="$(id -u)" -v executable="$app_program" '
      $1 == uid {
        pid=$2; $1=""; $2=""; sub(/^[[:space:]]+/, "")
        if ($0 == executable || $0 == executable " --after-upgrade") print pid
      }'
}
was_running=0
service_running=0
service_pids() {
  # Only this user's *installed* Slingring daemon. The socket may disappear
  # just before the process fully exits, so wait for the executable to stop
  # before replacing its signed bytes during an upgrade.
  /bin/ps -axww -o uid= -o pid= -o command= 2>/dev/null |
    /usr/bin/awk -v uid="$(id -u)" -v executable="$target" '
      $1 == uid {
        pid=$2; $1=""; $2=""; sub(/^[[:space:]]+/, "")
        if ($0 == executable " daemon" || $0 == executable " daemon --ephemeral") print pid
      }'
}
if [ "$action" = upgrade ]; then
  state=$("$target" status --json)
  if printf '%s' "$state" | grep -Eq '"active_tasks":[1-9][0-9]*'; then
    echo 'A task is active. Let it finish before upgrading; installation unchanged.' >&2; exit 1
  fi
  if printf '%s' "$state" | grep -q '"running":true'; then
    service_running=1
    "$target" service stop >/dev/null
  fi
  # Also protect against a non-responsive or still-starting old process: the
  # CLI socket alone cannot prove the signed executable has fully exited.
  attempt=0
  while [ -n "$(service_pids)" ] && [ "$attempt" -lt 120 ]; do
    sleep 0.25
    attempt=$((attempt + 1))
  done
  if [ -n "$(service_pids)" ]; then
    echo 'Slingring daemon did not fully exit; installation unchanged. Try upgrading again when it has stopped.' >&2
    exit 1
  fi
fi
if [ -n "$(app_pids)" ]; then
  was_running=1
  for pid in $(app_pids); do kill -TERM "$pid" 2>/dev/null || true; done
  for attempt in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
    [ -z "$(app_pids)" ] && break
    sleep 0.1
  done
  [ -z "$(app_pids)" ] || { echo 'Menu bar app did not exit; nothing was replaced' >&2; exit 1; }
fi
if [ -d "$app_dest" ]; then mv "$app_dest" "$backup/old-app"; fi
if [ -f "$target" ]; then mv "$target" "$backup/old-cli"; fi
restore() {
  rm -rf "$app_dest"
  rm -f "$target"
  [ ! -d "$backup/old-app" ] || mv "$backup/old-app" "$app_dest"
  [ ! -f "$backup/old-cli" ] || mv "$backup/old-cli" "$target"
  if [ "$was_running" = 1 ] && [ -d "$app_dest" ]; then
    /usr/bin/open -n -g "$app_dest" --args --after-upgrade >/dev/null 2>&1 || true
  fi
  if [ "$service_running" = 1 ]; then "$target" service start >/dev/null 2>&1 || true; fi
}
if ! mv "$staged/$app" "$app_dest" || ! mv "$staged/$cli" "$target"; then
  restore; echo 'Install failed; restored previous CLI and app' >&2; exit 1
fi
if ! codesign --verify --strict "$target" || ! codesign --verify --deep --strict "$app_dest" || ! "$target" --version; then
  restore; echo 'Installed app or CLI failed verification; restored previous release' >&2; exit 1
fi
rm -rf "$backup/old-app"
rm -f "$backup/old-cli"
printf 'Installed %s and %s at %s\n' "$cli" "$app" "$install_dir"
# New installs default both login settings to on. For upgrades, the CLI
# migrates legacy service-uninstall choices and leaves managed opt-outs alone.
# Registration failures (including macOS approval requirements) are reported
# without rolling back an otherwise verified CLI/app replacement.
if [ "$action" = upgrade ]; then
  if ! "$target" login setup --upgrade; then
    echo 'Login settings were not fully configured. Inspect with: slingring login status' >&2
  fi
else
  if ! "$target" login setup; then
    echo 'Login settings were not fully configured. Inspect with: slingring login status' >&2
  fi
fi
if [ "${CLI_NO_LAUNCH:-0}" != 1 ] && { [ "$was_running" = 1 ] || [ "${CLI_LAUNCH_MENU_BAR:-1}" = 1 ]; }; then
  /usr/bin/open -n -g "$app_dest" --args --after-upgrade ||
    echo "Installed successfully, but could not launch $app; open it manually." >&2
fi
if [ "$service_running" = 1 ]; then
  "$target" service start >/dev/null ||
    echo 'Updated successfully, but the service did not restart. Run slingring service start.' >&2
fi

if [ "$action" = install ]; then
  printf '\nChoose the Chrome profile signed into ChatGPT:\n'
  printf '  Menu bar → Configure → Chrome Profile\n'
  printf '  Or: %s profile list\n      %s profile use "PROFILE NAME"\n' "$target" "$target"
  printf '\nStart a chat:\n  %s new-task "Your prompt"\n' "$target"
fi
