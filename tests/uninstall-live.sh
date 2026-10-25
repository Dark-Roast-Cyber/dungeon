#!/bin/sh
# Destructive acceptance gate: run only as a disposable Linux test user/VM.
set -eu
umask 077

fail() { printf '%s\n' "uninstall-live: $*" >&2; exit 1; }
[ "${DUNGEON_UNINSTALL_TEST:-}" = disposable-user ] || fail 'set DUNGEON_UNINSTALL_TEST=disposable-user in a disposable VM/user only'
[ "$(id -u)" != 0 ] || fail 'rootless test user required'
root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd -P)
config=${XDG_CONFIG_HOME:-$HOME/.config}/dungeon
state=${XDG_STATE_HOME:-$HOME/.local/state}/dungeon
registry=${XDG_CONFIG_HOME:-$HOME/.config}/openshell/gateways
image=${DUNGEON_UNINSTALL_TEST_IMAGE:-localhost/dungeon-agent:0.0.3-opencode-2.1.2}
for path in "$config" "$state" "$registry/dungeon-local" "$registry/uninstall-sentinel" "$HOME/.local/bin/dungeon"; do
    [ ! -e "$path" ] && [ ! -L "$path" ] || fail 'test requires no pre-existing Dungeon or sentinel state'
done
containers=$(podman ps --all --quiet) || fail 'container inventory failed'
[ -z "$containers" ] || fail 'test requires an initially empty container inventory'
if podman network exists dungeon-local; then fail 'dungeon-local network already exists'; else [ "$?" = 1 ] || fail 'network query failed'; fi
podman image exists "$image" || fail 'build the reviewed agent image explicitly first'
podman image exists 'ghcr.io/nvidia/openshell/gateway@sha256:05cf77bbb022a739aed6f22daa0e7e164415f4ab273b5f84319e46d91eb8f645' || fail 'pull the pinned gateway image explicitly first'

fixture=$(mktemp -d "$HOME/dungeon-uninstall-test.XXXXXXXX")
printf 'Evidence and preserved projects: %s\n' "$fixture"
# Deliberately retain evidence and runtime on failure; never hide incomplete cleanup.
if [ -n "${DUNGEON_UNINSTALL_TEST_BINARY:-}" ]; then
    # Native-archive path: the test runner supplies its independently built release binary.
    [ -f "$DUNGEON_UNINSTALL_TEST_BINARY" ] && [ ! -L "$DUNGEON_UNINSTALL_TEST_BINARY" ] || fail 'prebuilt binary must be a regular file'
    mkdir "$fixture/bin"
    install -m 755 "$DUNGEON_UNINSTALL_TEST_BINARY" "$fixture/bin/dungeon"
else
    "$root/scripts/install-local.sh" --bin-dir "$fixture/bin"
fi
dungeon=$fixture/bin/dungeon
openshell gateway add http://127.0.0.1:9 --local --name uninstall-sentinel
openshell gateway list --output json > "$fixture/gateways-before.json"
sentinel=$(podman run -d --network none --entrypoint /bin/sleep "$image" 3600)
podman inspect "$sentinel" --format '{{.Id}} {{.State.Status}} {{.State.StartedAt}}' > "$fixture/sentinel-before"
podman volume ls --format '{{.Name}}' > "$fixture/volumes-before.raw"
sort "$fixture/volumes-before.raw" > "$fixture/volumes-before"
podman secret ls --format '{{.ID}}' > "$fixture/secrets-before.raw"
sort "$fixture/secrets-before.raw" > "$fixture/secrets-before"
mkdir "$fixture/live-project" "$fixture/copy-project"
printf 'preserve live host project\n' > "$fixture/live-project/host.txt"
printf 'preserve copy host project\n' > "$fixture/copy-project/host.txt"
sha256sum "$fixture/live-project/host.txt" "$fixture/copy-project/host.txt" > "$fixture/host-before.sha256"
cd "$fixture/live-project"
"$dungeon" setup
"$dungeon" init --yes --agent opencode --harness-config disabled --project-transfer live --image "$image"
"$dungeon" up
"$dungeon" exec -- /bin/sh -c 'printf "guest state\n" > /sandbox/uninstall-marker'
cd "$fixture/copy-project"
"$dungeon" init --yes --agent opencode --harness-config disabled --project-transfer copy --image "$image"
"$dungeon" up
"$dungeon" down
podman ps --all --no-trunc --filter network=dungeon-local --format '{{.ID}}' > "$fixture/dungeon-containers-before"
[ "$(wc -l < "$fixture/dungeon-containers-before")" -eq 3 ] || fail 'expected two sandbox containers and one gateway'
podman ps --all --no-trunc --format '{{.ID}} {{.State}} {{.StartedAt}}' > "$fixture/dry-before"
"$dungeon" uninstall --bin-dir "$fixture/bin" --dry-run > "$fixture/removal-plan.json"
[ -f "$dungeon" ] && [ -d "$config" ] && [ -d "$state" ] || fail 'dry-run changed installation'
podman ps --all --no-trunc --format '{{.ID}} {{.State}} {{.StartedAt}}' > "$fixture/dry-after"
cmp "$fixture/dry-before" "$fixture/dry-after"
"$dungeon" uninstall --bin-dir "$fixture/bin" --yes > "$fixture/removal.log" 2>&1

for path in "$dungeon" "$config" "$state" "$registry/dungeon-local"; do
    [ ! -e "$path" ] && [ ! -L "$path" ] || fail 'installation or private state remains'
done
while IFS= read -r container; do
    if podman container exists "$container"; then fail 'Dungeon container remains'; else [ "$?" = 1 ] || fail 'container absence query failed'; fi
done < "$fixture/dungeon-containers-before"
if podman network exists dungeon-local; then fail 'Dungeon network remains'; else [ "$?" = 1 ] || fail 'network absence query failed'; fi
podman ps --all --no-trunc --format '{{.ID}}' > "$fixture/containers-after"
[ "$(cat "$fixture/containers-after")" = "$sentinel" ] || fail 'unexpected remaining/missing container'
podman inspect "$sentinel" --format '{{.Id}} {{.State.Status}} {{.State.StartedAt}}' > "$fixture/sentinel-after"
cmp "$fixture/sentinel-before" "$fixture/sentinel-after"
openshell gateway list --output json > "$fixture/gateways-after.json"
cmp "$fixture/gateways-before.json" "$fixture/gateways-after.json"
podman volume ls --format '{{.Name}}' > "$fixture/volumes-after.raw"
sort "$fixture/volumes-after.raw" > "$fixture/volumes-after"
podman secret ls --format '{{.ID}}' > "$fixture/secrets-after.raw"
sort "$fixture/secrets-after.raw" > "$fixture/secrets-after"
cmp "$fixture/volumes-before" "$fixture/volumes-after"
cmp "$fixture/secrets-before" "$fixture/secrets-after"
sha256sum --check "$fixture/host-before.sha256"
podman image exists "$image"
openshell --version > "$fixture/openshell-version"
podman --version > "$fixture/podman-version"
# Reinstall in the default location, then remove a binary-only installation too.
if [ -n "${DUNGEON_UNINSTALL_TEST_BINARY:-}" ]; then
    mkdir -p "$HOME/.local/bin"
    install -m 755 "$DUNGEON_UNINSTALL_TEST_BINARY" "$HOME/.local/bin/dungeon"
else
    "$root/scripts/install-local.sh"
fi
"$HOME/.local/bin/dungeon" uninstall --yes > "$fixture/binary-only-removal.log" 2>&1
for path in "$HOME/.local/bin/dungeon" "$config" "$state"; do
    [ ! -e "$path" ] && [ ! -L "$path" ] || fail 'binary-only reinstall/uninstall left state'
done
# Remove only the two explicitly created unrelated sentinel resources.
podman rm --force "$sentinel"
openshell gateway remove uninstall-sentinel
rmdir "$registry/uninstall-sentinel"
printf 'PASS: containers, installation, private state and gateway removed; projects and unrelated resources preserved. Evidence: %s\n' "$fixture"
