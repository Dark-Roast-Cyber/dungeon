const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const vm = require('node:vm');
const { test } = require('node:test');
const { directories, verifyLayout } = require('./verify-layout.cjs');
const manifest = require('./package.json');

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dungeon-image-layout-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const directory of directories) fs.mkdirSync(path.join(root, directory), { mode: 0o700 });
  return root;
}

test('final layout is empty and writable without launching a harness', t => {
  const root = fixture(t);
  verifyLayout(root, process.getuid(), process.getgid());
  for (const directory of directories.filter(p => p.endsWith('/opencode'))) {
    assert.deepEqual(fs.readdirSync(path.join(root, directory)), []);
  }
});

test('layout refuses missing, wrong-mode and nonempty native directories', t => {
  const root = fixture(t);
  const target = path.join(root, '.local/share/opencode');
  fs.rmdirSync(target);
  assert.throws(() => verifyLayout(root, process.getuid(), process.getgid()));
  fs.mkdirSync(target, { mode: 0o755 });
  assert.throws(() => verifyLayout(root, process.getuid(), process.getgid()), /0700/);
  fs.chmodSync(target, 0o700);
  fs.writeFileSync(path.join(target, 'synthetic-state'), 'synthetic');
  assert.throws(() => verifyLayout(root, process.getuid(), process.getgid()), /empty/);
});

test('layout refuses redirected parents and incorrect ownership', t => {
  const root = fixture(t);
  assert.throws(() => verifyLayout(root, process.getuid() + 1, process.getgid()), /sandbox-owned/);
  assert.throws(() => verifyLayout(root, process.getuid(), process.getgid() + 1), /sandbox-group/);
  fs.renameSync(path.join(root, '.local'), path.join(root, 'redirect'));
  fs.symlinkSync('redirect', path.join(root, '.local'));
  assert.throws(() => verifyLayout(root, process.getuid(), process.getgid()), /not a link/);
});

const source = fs.readFileSync(path.join(__dirname, 'verify.cjs'), 'utf8');
const agentsReached = new Error('runtime accepted; agent checks reached');

function verifyRuntime({ version = manifest.imageTools.node.version, execPath = '/usr/bin/node',
  uid = 998, owner = 0, redirected, writable, mode = 0o755, reportedVersion = version } = {}) {
  const aliases = new Map([
    ['/usr/bin/node', '/usr/bin/node'], ['/usr/bin/nodejs', '/usr/bin/node'],
    ['/bin/node', '/usr/bin/node'], ['/bin/nodejs', '/usr/bin/node'],
  ]);
  if (redirected) aliases.set(...redirected);
  const fakeFs = {
    constants: fs.constants,
    existsSync: pathname => aliases.has(pathname),
    realpathSync: pathname => aliases.get(pathname),
    statSync: () => ({ uid: owner, mode }),
    accessSync(pathname, flags) {
      if (flags === fs.constants.W_OK && pathname !== writable) throw new Error('EACCES');
      if (flags === fs.constants.X_OK && !aliases.has(pathname)) throw new Error('ENOENT');
    },
  };
  vm.runInNewContext(source, {
    __dirname,
    console: { log() {} },
    process: {
      argv: ['node', 'verify.cjs', '--path'], platform: 'linux', arch: 'x64',
      versions: { node: version }, execPath, getuid: () => uid,
      env: { PATH: '/usr/local/bin:/usr/bin:/bin' },
    },
    require(name) {
      if (name === './package.json') return manifest;
      if (name === 'node:fs') return fakeFs;
      if (name === 'node:child_process') return {
        execFileSync(binary) {
          if (binary === '/usr/bin/node') return `v${reportedVersion}\n`;
          throw agentsReached;
        },
      };
      return require(name);
    },
  });
}

test('runtime accepts the pinned binary and aliases before agent checks', () => {
  assert.throws(() => verifyRuntime(), error => error === agentsReached);
});

test('runtime rejects stale executing or exact-path Node versions', () => {
  assert.throws(() => verifyRuntime({ version: '22.22.1' }), /22\.22\.1/);
  assert.throws(() => verifyRuntime({ reportedVersion: '22.22.1' }), /22\.22\.1/);
  assert.throws(() => verifyRuntime({ execPath: '/tmp/node' }), /\/tmp\/node/);
});

test('runtime rejects redirected exact paths, PATH shadows and alternate aliases', () => {
  for (const pathname of ['/usr/bin/node', '/usr/local/bin/node', '/usr/local/bin/nodejs', '/usr/bin/nodejs']) {
    assert.throws(() => verifyRuntime({ redirected: [pathname, '/tmp/stale-node'] }), /reviewed/);
  }
});

test('runtime rejects root execution and mutable binary or parent', () => {
  assert.throws(() => verifyRuntime({ uid: 0 }), /non-root/);
  assert.throws(() => verifyRuntime({ owner: 998 }), /root-owned/);
  for (const writable of ['/usr/bin/node', '/usr/bin']) {
    assert.throws(() => verifyRuntime({ writable }), /must not be writable/);
  }
  assert.throws(() => verifyRuntime({ mode: 0o775 }), /group\/other-writable/);
});
