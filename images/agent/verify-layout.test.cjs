const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');
const { directories, verifyLayout } = require('./verify-layout.cjs');

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
