const assert = require('node:assert/strict');
const fs = require('node:fs');
const { execFileSync } = require('node:child_process');
const path = require('node:path');
const { dependencies, imageTools } = require('./package.json');

const verifyRuntimePath = process.argv.includes('--path');

for (const [command, name, prefix, suffix] of [
  ['opencode', '@opencode/cli', 'opencode v', ''],
  ['claude', '@anthropic-ai/claude-code', '', ' (Claude Code)'],
  ['codex', '@openai/codex', 'codex-cli ', ''],
]) {
  const binary = verifyRuntimePath
    ? command
    : path.join(__dirname, 'node_modules', '.bin', command);
  const actual = execFileSync(binary, ['--version'], {
    encoding: 'utf8',
    timeout: 30000,
  }).trim();
  assert.equal(actual, `${prefix}${dependencies[name]}${suffix}`);
  console.log(actual);
}

if (!verifyRuntimePath) process.exit(0);

function findOnPath(command) {
  for (const directory of process.env.PATH.split(path.delimiter)) {
    const candidate = path.join(directory, command);
    try {
      fs.accessSync(candidate, fs.constants.X_OK);
      return candidate;
    } catch {}
  }
  assert.fail(`${command} is not executable on PATH`);
}

function verifyImmutable(pathname) {
  const stat = fs.statSync(pathname);
  assert.equal(stat.uid, 0, `${pathname} must be root-owned`);
  assert.equal(stat.mode & 0o022, 0, `${pathname} must not be group/other-writable`);
  assert.throws(
    () => fs.accessSync(pathname, fs.constants.W_OK),
    `${pathname} must not be writable by sandbox`,
  );
}

function verifyImageOwnedPath(command, expected) {
  const resolved = findOnPath(command);
  assert.equal(resolved, expected, `${command} resolved outside the reviewed path`);
  assert.equal(
    fs.realpathSync(resolved),
    expected,
    `${command} reviewed path must not redirect elsewhere`,
  );
  verifyImmutable(expected);
}

verifyImageOwnedPath('nvim', '/usr/local/bin/nvim');
verifyImageOwnedPath('rustc', '/usr/local/bin/rustc');
verifyImageOwnedPath('cargo', '/usr/local/bin/cargo');

const nvimVersion = execFileSync('/usr/local/bin/nvim', ['--version'], {
  encoding: 'utf8',
  timeout: 30000,
}).split('\n')[0];
assert.equal(nvimVersion, `NVIM v${imageTools.neovim.version}`);
console.log(nvimVersion);

for (const command of ['rustc', 'cargo']) {
  const actual = execFileSync(`/usr/local/bin/${command}`, ['--version'], {
    encoding: 'utf8',
    timeout: 30000,
  }).trim();
  assert.equal(actual.split(/\s+/)[1], imageTools.rust.version);
  console.log(actual);
}

for (const pathname of [
  '/usr',
  '/usr/local',
  '/usr/lib/node_modules/dungeon-agent',
  '/usr/local/lib/nvim',
  '/usr/local/share/nvim',
  '/usr/local/lib/rustlib',
]) {
  verifyImmutable(pathname);
}

for (const [name, expected] of Object.entries({
  HOME: '/sandbox',
  XDG_CONFIG_HOME: '/sandbox/.config',
  XDG_DATA_HOME: '/sandbox/.local/share',
  XDG_STATE_HOME: '/sandbox/.local/state',
  XDG_CACHE_HOME: '/sandbox/.cache',
  CARGO_HOME: '/sandbox/.cargo',
})) {
  assert.equal(process.env[name], expected, `${name} must remain guest-private`);
}

assert.notEqual(process.getuid(), 0, 'runtime user must be non-root');

for (const socket of [
  '/var/run/docker.sock',
  '/run/docker.sock',
  '/run/podman/podman.sock',
  '/run/user/998/podman/podman.sock',
]) {
  assert.equal(fs.existsSync(socket), false, `engine socket must be absent: ${socket}`);
}

for (const credentialPath of [
  '/sandbox/.codex/auth.json',
  '/sandbox/.config/gh/hosts.yml',
  '/sandbox/.cargo/credentials',
  '/sandbox/.cargo/credentials.toml',
  '/sandbox/.npmrc',
  '/sandbox/.ssh/id_rsa',
  '/sandbox/.ssh/id_ed25519',
]) {
  assert.equal(
    fs.existsSync(credentialPath),
    false,
    `credential/session state must be absent: ${credentialPath}`,
  );
}
