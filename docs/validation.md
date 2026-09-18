# Validation and release boundary

## Completed locally for 0.0.1

28 offline tests passed, covering strict/duplicate-key configuration handling,
private configuration placement, starter policies, fingerprint stability and
change detection, provider-name validation, no implicit project execution,
command arguments, host environment filtering, explicit upload consent,
sandbox identity/replacement checks, policy drift, readiness failures,
idempotent creation, creation failure, exit codes, version mismatch, outward
symlink rejection, and the Telltale dry-run command boundary.

A wheel was built and installed into a separate virtual environment. The CLI
version/help and the same test suite were checked against that installation.
Current main additionally tests that Codex authentication dispatches only the
vendor device-code command, Claude authentication stays inside the vendor CLI,
and unsupported auth integrations are rejected at the parser boundary. The tests
fake OpenShell responses. They are not recordings of successful real
deployments, successful OAuth exchanges, or proof that backend policy is enforced.

The GitHub workflow checks offline tests and wheel packaging on Linux, macOS,
and Windows. One symlink test is skipped on Windows because creating symlinks
can require additional privileges. A passing Windows manager test is not native
Windows agent containment validation. Read the actual run before making a CI
success claim.

## Required live gates before broader support claims

| Gate | Evidence required |
| --- | --- |
| Runtime | Exact CLI, gateway, driver, kernel and image digest; clean create/start/stop. |
| Enforcement | Denied path, unapproved egress, raw network bypass, and privilege attempt; observed denial and logs. |
| CLI agents | Each initial agent authenticates, edits/tests a fixture, resizes, handles interrupts, exits, and resumes. |
| Persistence | Project/session survival across stop/restart; behavior after driver failure and deletion documented. |
| Upload | Destination layout, Git filtering, symlinks, dotfiles, partial failure and overwrite behavior. |
| Remote interfaces | VS Code and each desktop client connect, use the guest path, and reveal credential/execution boundaries. |
| Telltale | Correct source discovery, positive/negative fixtures and failure reporting; durable external collection remains separate work. |
| Endpoint protection | Supported sensor deployment observes and controls a known guest test process, with correct environment attribution. |

No gateway, container engine, virtual machine, live desktop client, or endpoint
sensor was available in the implementation environment. These gates are pending.
Do not translate them into checked boxes merely because upstream advertises a
capability.

## Release procedure

Normal commits and pull requests run CI without publishing. An authorized main
commit with the exact subject `release: v<package-version>` additionally runs the
release job after all offline checks pass. That job builds source/wheel archives,
adds SHA-256 checksums, and creates a GitHub **prerelease** at that exact commit.
It never overwrites an existing release or publishes to PyPI. The commit subject
is an explicit release action, not a convention to use for ordinary changes.

Before a release, inspect the staged diff, run the tests, install the built
wheel in a clean environment, and verify the version. After publication, verify
the target commit, checksums, and asset list. Do not reuse a version after a
validation-relevant published artifact change.
