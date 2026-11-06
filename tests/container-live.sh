#!/bin/sh
# Run with a disposable HOME and a local native engine; only this fixture is removed.
set -eu
umask 077
test "${DUNGEON_CONTAINER_TEST:-}" = disposable-home || { echo 'set DUNGEON_CONTAINER_TEST=disposable-home with a disposable HOME' >&2; exit 1; }
binary=${DUNGEON_CONTAINER_BINARY:?absolute Dungeon binary required}
engine=${DUNGEON_CONTAINER_ENGINE:?podman or docker required}
endpoint=${DUNGEON_CONTAINER_ENDPOINT:?explicit local engine socket required}
image=${DUNGEON_CONTAINER_IMAGE:-ghcr.io/nvidia/openshell-community/sandboxes/base@sha256:aeef1c63f00e2913ea002ccb3aaf925f338b5c5d70e63576f0d95c16a138044e}
test ! -e "$HOME/.config/dungeon"
test ! -e "$HOME/.local/state/dungeon"
mkdir -p "$HOME/project" "$HOME/bin"
mkdir -p "$HOME/.config/opencode/themes"
printf '{"fixture":"read-only"}\n' > "$HOME/.config/opencode/themes/dungeon-fixture.json"
sha256sum "$HOME/.config/opencode/themes/dungeon-fixture.json" > "$HOME/import-before.sha256"
install -m 755 "$binary" "$HOME/bin/dungeon"
dungeon=$HOME/bin/dungeon
cd "$HOME/project"
printf 'host\n' > host.txt
"$dungeon" init --yes --agent opencode --runtime container --engine "$engine" --endpoint "$endpoint" --image "$image" --harness-config read-only
"$dungeon" reload --harness-config copy --yes
"$dungeon" plan > "$HOME/plan.json"
"$dungeon" run opencode -- --version > "$HOME/first-version.txt"
test "$(wc -l < "$HOME/first-version.txt")" -eq 1
"$dungeon" up
"$dungeon" status > "$HOME/status-before.json"
"$dungeon" exec -- /bin/sh -c 'test "$(id -u)" != 0; test "$(cat /sandbox/project/host.txt)" = host; printf guest > /sandbox/project/guest.txt; printf retained > "$HOME/session-marker"'
"$dungeon" exec -- /bin/sh -c 'grep read-only /sandbox/.config/opencode/themes/dungeon-fixture.json; if (echo forbidden > /sandbox/.config/opencode/themes/dungeon-fixture.json) 2>/dev/null; then exit 1; fi; command -v opencode claude codex'
"$dungeon" run opencode -- --version
"$dungeon" run claude -- --version
"$dungeon" run codex -- --version
printf 'headless stdin\n' > "$HOME/stdin.txt"
"$dungeon" exec -- cat < "$HOME/stdin.txt" > "$HOME/stdout.txt"
cmp "$HOME/stdin.txt" "$HOME/stdout.txt"
"$dungeon" exec -- /bin/sh -c 'test "$1" = "two words; \$(not-a-command)"' dungeon 'two words; $(not-a-command)'
if "$dungeon" exec -- /bin/sh -c 'printf stdout; printf stderr >&2; exit 23' > "$HOME/command.stdout" 2> "$HOME/command.stderr"; then
    echo 'expected workload exit 23' >&2
    exit 1
else
    test "$?" -eq 23
fi
test "$(cat "$HOME/command.stdout")" = stdout
test "$(cat "$HOME/command.stderr")" = stderr
test "$(cat guest.txt)" = guest
"$dungeon" down
"$dungeon" run opencode -- --version > "$HOME/resumed-version.txt"
cmp "$HOME/first-version.txt" "$HOME/resumed-version.txt"
"$dungeon" exec -- /bin/sh -c 'test "$(cat "$HOME/session-marker")" = retained'
"$dungeon" status > "$HOME/status-after.json"
cmp "$HOME/status-before.json" "$HOME/status-after.json"
"$dungeon" uninstall --bin-dir "$HOME/bin" --dry-run > "$HOME/uninstall-plan.json"
test -f "$dungeon"
"$dungeon" uninstall --bin-dir "$HOME/bin" --yes
test ! -e "$dungeon"
test ! -e "$HOME/.config/dungeon"
test "$(cat host.txt)" = host
test "$(cat guest.txt)" = guest
sha256sum --check "$HOME/import-before.sha256"
printf 'PASS: native %s headless I/O, lifecycle, project writes, state persistence and uninstall\n' "$engine"
