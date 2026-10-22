#!/bin/sh
set -eu
umask 077

usage() {
    cat <<'EOF'
Usage: scripts/install-local.sh [--bin-dir ABSOLUTE_PATH] [--replace] [--dry-run]

Build this checkout's Rust binary with the locked dependency graph and install
it as dungeon in a user-owned bin directory (default: ~/.local/bin). Identical
installs are a no-op. A different existing binary requires --replace.

This does not install prerequisites, set PATH, or touch OpenShell/Podman state.
EOF
}

fail() { printf '%s\n' "install-local: $*" >&2; exit 1; }

replace=false
dry_run=false
bin_dir=${HOME:?HOME is required}/.local/bin
while [ "$#" -gt 0 ]; do
    case "$1" in
        --replace) replace=true ;;
        --dry-run) dry_run=true ;;
        --bin-dir)
            [ "$#" -ge 2 ] || fail '--bin-dir requires an absolute path'
            bin_dir=$2
            shift ;;
        --help|-h) usage; exit 0 ;;
        *) fail "unknown option: $1" ;;
    esac
    shift
done

case "$bin_dir" in /*) ;; *) fail '--bin-dir must be absolute' ;; esac
[ "$(id -u)" != 0 ] || fail 'run as your normal user, not root'
root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
[ -f "$root/Cargo.toml" ] || fail 'run the helper from a Dungeon source checkout'
[ ! -L "$bin_dir" ] || fail 'bin directory must not be a symlink'
[ ! -e "$bin_dir" ] || [ -d "$bin_dir" ] || fail 'bin path is not a directory'

if [ "$dry_run" = true ]; then
    printf 'Would build %s with cargo build --release --locked and install in %s\n' "$root" "$bin_dir"
    printf '%s\n' 'No build, installation, gateway, or container changes made.'
    exit 0
fi

if [ ! -d "$bin_dir" ]; then
    mkdir -p -- "$bin_dir"
fi
bin_dir=$(CDPATH='' cd -- "$bin_dir" && pwd -P)
case "$bin_dir/" in "$root/"*) fail 'keep the installed binary outside the Dungeon checkout' ;; esac
[ "$(stat -c %u -- "$bin_dir")" = "$(id -u)" ] || fail 'bin directory must be owned by the current user'
destination=$bin_dir/dungeon
if [ -L "$destination" ]; then fail 'refusing a symlink at the install destination'; fi
if [ -e "$destination" ]; then
    [ -f "$destination" ] || fail 'install destination is not a regular file'
    [ "$(stat -c %u -- "$destination")" = "$(id -u)" ] || fail 'installed binary belongs to another user'
fi

(cd -- "$root" && cargo build --release --locked)
source=$root/target/release/dungeon
[ -f "$source" ] && [ ! -L "$source" ] || fail 'release build did not produce a regular binary'
case "$("$source" --version)" in 'dungeon '[0-9]*) ;; *) fail 'release binary failed its version check' ;; esac
[ ! -L "$destination" ] || fail 'install destination changed to a symlink during build'

if [ -e "$destination" ] && cmp -s -- "$source" "$destination"; then
    printf 'Already installed: %s\n' "$destination"
    exit 0
fi
if [ -e "$destination" ] && [ "$replace" != true ]; then
    fail "different binary at $destination; inspect it and rerun with --replace to approve replacement"
fi

temporary=$(mktemp "$bin_dir/.dungeon.XXXXXXXX")
trap 'rm -f -- "$temporary"' EXIT
trap 'exit 1' HUP INT TERM
cp -- "$source" "$temporary"
chmod 755 -- "$temporary"

if [ "$replace" = true ]; then
    [ ! -L "$destination" ] || fail 'install destination changed to a symlink'
    [ ! -e "$destination" ] || { [ -f "$destination" ] && [ "$(stat -c %u -- "$destination")" = "$(id -u)" ]; } || fail 'install destination changed during build'
    mv -fT -- "$temporary" "$destination"
else
    ln -- "$temporary" "$destination" 2>/dev/null || fail 'install destination appeared during build; nothing replaced'
    rm -f -- "$temporary"
fi
trap - EXIT HUP INT TERM
printf 'Installed %s\n' "$destination"
printf '%s\n' 'Add the bin directory to PATH if needed, then run dungeon doctor; setup is separate.'
