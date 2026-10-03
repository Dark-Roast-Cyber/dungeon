'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { randomBytes } = require('node:crypto');
const { spawn, spawnSync } = require('node:child_process');
const shim = fs.readFileSync('src/owned.cjs', 'utf8');
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
function control(mode, token) {
  return spawnSync(process.execPath, ['-e', shim, mode, token], { encoding: 'utf8', timeout: 2000 });
}
function launch(argv) {
  const token = randomBytes(32).toString('hex');
  const child = spawn(process.execPath, ['-e', shim, 'launch', token, ...argv], { stdio: ['ignore', 'pipe', 'pipe'] });
  let stdout = '';
  child.stdout.on('data', data => stdout += data);
  const done = new Promise(resolve => child.once('exit', (code, signal) => resolve({ code, signal, stdout })));
  return { token, child, done };
}
async function ready(token) {
  for (let i = 0; i < 100; i++) {
    if (fs.existsSync(`/tmp/dungeon-owned-${token}/stop`)) return;
    await pause(20);
  }
  throw Error('startup timed out');
}
function finish(token, code) {
  const result = control('result', token);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), { token, code });
  assert.equal(control('remove', token).status, 0);
  assert.equal(fs.existsSync(`/tmp/dungeon-owned-${token}`), false);
}
test('argv is positional, no inherited stdin, nonzero exit and lingering descendant cleaned', { timeout: 6000 }, async () => {
  const argv = ['', 'a b', "'\";$(touch /tmp/never-owned)", '--flag'];
  const code = `const fs=require('fs'); const {spawn}=require('child_process'); const child=spawn(process.execPath,['-e','process.on("SIGTERM",()=>{});setInterval(()=>{},1000)'],{stdio:'inherit'}); console.log(JSON.stringify(process.argv.slice(1))); console.log(child.pid); if(process.channel || process.env.NODE_CHANNEL_FD)process.exit(98);for(const fd of fs.readdirSync('/proc/self/fd')){try{if(fs.readlinkSync('/proc/self/fd/'+fd).includes('dungeon-owned-'))process.exit(99)}catch{}};if(fs.readFileSync(0,'utf8')!=='')process.exit(97);setTimeout(()=>process.exit(23),200)`;
  const run = launch([process.execPath, '-e', code, ...argv]);
  const result = await run.done;
  assert.equal(result.code, 23);
  assert.equal(result.stdout.split('\n')[0], JSON.stringify(argv));
  const pid = Number(result.stdout.split('\n')[1]);
  try {
    const stat = fs.readFileSync(`/proc/${pid}/stat`, 'utf8');
    assert.equal(stat.slice(stat.lastIndexOf(')') + 2).split(' ')[0], 'Z', 'descendant is still running');
  } catch (error) { if (error.code !== 'ENOENT') throw error; }
  finish(run.token, 23);
});
test('missing executable preserves native command failure', { timeout: 4000 }, async () => {
  const run = launch(['/no-such-owned-executable']);
  assert.equal((await run.done).code, 127);
  finish(run.token, 127);
});
test('concurrent invocations and unrelated sentinel are isolated; resistant child is killed', { timeout: 8000 }, async () => {
  const sentinel = spawn(process.execPath, ['-e', 'setInterval(()=>{},1000)'], { stdio: 'ignore' });
  const a = launch([process.execPath, '-e', "process.on('SIGTERM',()=>{});setInterval(()=>{},1000)"]);
  const b = launch([process.execPath, '-e', 'setInterval(()=>{},1000)']);
  try {
    await Promise.all([ready(a.token), ready(b.token)]);
    await pause(150);
    assert.equal(control('stop', a.token).status, 0);
    assert.equal(control('stop', a.token).status, 0);
    await a.done;
    finish(a.token, 143);
    assert.equal(b.child.exitCode, null);
    assert.equal(sentinel.exitCode, null);
    assert.equal(control('stop', b.token).status, 0);
    await b.done;
    finish(b.token, 143);
  } finally { sentinel.kill(); }
});
test('early STOP prevents or terminates launch, absent/stale controls never succeed', { timeout: 6000 }, async () => {
  const run = launch([process.execPath, '-e', 'setInterval(()=>{},1000)']);
  assert.notEqual(control('stop', '0'.repeat(64)).status, 0);
  await ready(run.token);
  assert.equal(control('stop', run.token).status, 0);
  await run.done;
  finish(run.token, 143);
  assert.notEqual(control('stop', run.token).status, 0);
  assert.notEqual(control('result', run.token).status, 0);
});
test('existing directory and forged symlink controls refuse without modifying target', { timeout: 4000 }, async () => {
  const token = randomBytes(32).toString('hex');
  const dir = `/tmp/dungeon-owned-${token}`;
  const target = `/tmp/dungeon-owned-target-${token}`;
  fs.mkdirSync(dir, { mode: 0o700 });
  fs.writeFileSync(target, 'preserved', { mode: 0o600 });
  try {
    fs.symlinkSync(target, `${dir}/stop`);
    assert.notEqual(control('stop', token).status, 0);
    assert.equal(fs.readFileSync(target, 'utf8'), 'preserved');
    const run = spawnSync(process.execPath, ['-e', shim, 'launch', token, '/bin/true'], { timeout: 2000 });
    assert.notEqual(run.status, 0);
  } finally { fs.unlinkSync(`${dir}/stop`); fs.rmdirSync(dir); fs.unlinkSync(target); }
});
test('forged FIFO control does not block and oversized receipts refuse', { timeout: 4000 }, () => {
  const token = randomBytes(32).toString('hex');
  const dir = `/tmp/dungeon-owned-${token}`;
  fs.mkdirSync(dir, { mode: 0o700 });
  try {
    assert.equal(spawnSync('/usr/bin/mkfifo', ['-m', '600', `${dir}/stop`]).status, 0);
    assert.notEqual(control('stop', token).status, 0);
    fs.writeFileSync(`${dir}/result`, 'x'.repeat(129), { mode: 0o600 });
    assert.notEqual(control('result', token).status, 0);
  } finally { fs.unlinkSync(`${dir}/stop`); fs.unlinkSync(`${dir}/result`); fs.rmdirSync(dir); }
});
test('external guardian SIGKILL after STOP or primary status cannot confirm surviving members', { timeout: 12000 }, async () => {
  for (const afterStatus of [false, true]) {
    const js = `const child=require('child_process').spawn(process.execPath,['-e', 'process.on("SIGTERM",()=>{});setInterval(()=>{},1000)'],{stdio:'ignore'});console.log(JSON.stringify({guardian:process.ppid,member:child.pid}));${afterStatus ? 'setTimeout(()=>process.exit(23),300)' : 'setInterval(()=>{},1000)'}`;
    const run = launch([process.execPath, '-e', js]);
    let ids;
    const output = new Promise(resolve => run.child.stdout.once('data', data => resolve(JSON.parse(data.toString()))));
    try {
      ids = await output;
      await pause(afterStatus ? 450 : 150);
      if (!afterStatus) assert.equal(control('stop', run.token).status, 0);
      await pause(80);
      process.kill(ids.guardian, 'SIGKILL');
      const result = await run.done;
      assert.equal(result.code, 125);
      assert.equal(process.kill(ids.member, 0), true);
      assert.notEqual(control('result', run.token).status, 0);
    } finally {
      // Only test-created, still-live resistant member; never a group kill after reap.
      if (ids) process.kill(ids.member, 'SIGKILL');
      fs.unlinkSync(`/tmp/dungeon-owned-${run.token}/stop`);
      fs.rmdirSync(`/tmp/dungeon-owned-${run.token}`);
    }
  }
});
