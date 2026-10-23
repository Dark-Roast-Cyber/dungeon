const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const directories = [
  '.config', '.config/opencode',
  '.local', '.local/share', '.local/share/opencode',
  '.local/state', '.local/state/opencode',
  '.cache', '.cache/opencode',
];

function verifyLayout(root, uid, gid) {
  for (const directory of directories) {
    const pathname = path.join(root, directory);
    const stat = fs.lstatSync(pathname);
    assert(stat.isDirectory(), `${pathname} must be a directory, not a link`);
    assert.equal(stat.uid, uid, `${pathname} must be sandbox-owned`);
    assert.equal(stat.gid, gid, `${pathname} must be sandbox-group-owned`);
    assert.equal(stat.mode & 0o7777, 0o700, `${pathname} must have mode 0700`);
    fs.accessSync(pathname, fs.constants.R_OK | fs.constants.W_OK | fs.constants.X_OK);
    if (directory.endsWith('/opencode')) {
      assert.deepEqual(fs.readdirSync(pathname), [], `${pathname} must be empty`);
      const probe = path.join(pathname, '.dungeon-layout-probe');
      const fd = fs.openSync(probe, 'wx', 0o600);
      try {
        fs.writeFileSync(fd, '');
      } finally {
        fs.closeSync(fd);
        fs.unlinkSync(probe);
      }
    }
  }
}

module.exports = { directories, verifyLayout };

if (require.main === module) {
  assert.equal(process.env.HOME, '/sandbox', 'HOME must remain guest-private');
  assert.equal(process.getuid(), 998, 'layout must be checked as sandbox UID998');
  assert.equal(process.getgid(), 998, 'layout must be checked as sandbox GID998');
  verifyLayout('/sandbox', 998, 998);
  console.log('Empty guest-owned OpenCode configuration/data/state/cache layout verified');
}
