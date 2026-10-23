const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { test } = require('node:test');

for (const enabled of [false, true]) {
  test(`native identity probe refuses host execution (opt-in ${enabled})`, t => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dungeon-probe-guard-'));
    t.after(() => fs.rmSync(root, { recursive: true, force: true }));
    const result = spawnSync(process.execPath, [path.join(__dirname, 'probe-opencode-identity.cjs')], {
      env: { HOME: root, PATH: '', ...(enabled ? { DUNGEON_SYNTHETIC_PROBE: '1' } : {}) },
      cwd: root, encoding: 'utf8', timeout: 5000,
    });
    assert.equal(result.status, 1);
    assert.equal(result.stdout, '');
    assert.equal(result.stderr.trim(), 'Synthetic native identity compatibility probe failed (disposable-guard); shared sign-in is not established.');
    assert.deepEqual(fs.readdirSync(root), []);
  });
}
