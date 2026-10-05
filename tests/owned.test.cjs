'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const { randomBytes } = require('node:crypto');
const { spawn, spawnSync } = require('node:child_process');
const path = require('node:path');
const os = require('node:os');
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
test('STOP IPC write failure still waits for guardian exit and group absence', { timeout: 6000 }, async () => {
  const token = randomBytes(32).toString('hex');
  // Node routes send errors to the callback, or the child error event if absent.
  // Inject the closing-pipe race after delivering STOP to a real guardian.
  const injected = shim.replace('let sequence = 0;', `
    const originalSend = child.send.bind(child);
    let injected = false;
    child.send = (value, callback) => originalSend(value, error => {
      if (value === 'stop' && !injected) {
        injected = true;
        error = Object.assign(Error('injected closing IPC pipe'), { code: 'EPIPE' });
      }
      if (callback) callback(error);
      else if (error) child.emit('error', error);
    });
    let sequence = 0;`);
  assert.notEqual(injected, shim);
  const app = "console.log(process.ppid);setInterval(()=>{},1000)";
  const child = spawn(process.execPath, ['-e', injected, 'launch', token, process.execPath, '-e', app], { stdio: ['ignore', 'pipe', 'pipe'] });
  const done = new Promise(resolve => child.once('exit', (code, signal) => resolve({ code, signal })));
  const group = await new Promise(resolve => child.stdout.once('data', data => resolve(Number(data.toString().trim()))));
  const identity = fs.readFileSync(`/proc/${group}/stat`, 'utf8').split(') ')[1].split(' ')[19];
  try {
    assert.equal(control('stop', token).status, 0);
    assert.deepEqual(await done, { code: 143, signal: null });
    assert.throws(() => process.kill(-group, 0), { code: 'ESRCH' });
    finish(token, 143);
  } finally {
    // Assertion failures must not leave the real synthetic guardian running.
    try {
      const fields = fs.readFileSync(`/proc/${group}/stat`, 'utf8').split(') ')[1].split(' ');
      if (fields[19] === identity && Number(fields[2]) === group) process.kill(-group, 'SIGKILL');
    } catch (error) { if (!['ENOENT', 'ESRCH'].includes(error.code)) throw error; }
    await done;
    fs.rmSync(`/tmp/dungeon-owned-${token}`, { recursive: true, force: true });
  }
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
test('framed guardian faults terminate despite silent surviving pipe holders', { timeout: 20000 }, async t => {
  for (const phase of ['before-stop', 'after-stop', 'after-status']) {
    await t.test(phase, async () => {
      const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dungeon-guardian-'));
      const marker = path.join(root, 'identities');
      const term = path.join(root, 'term');
      const token = randomBytes(32).toString('hex');
      const identity = pid => {
        const fields = fs.readFileSync(`/proc/${pid}/stat`, 'utf8').split(') ')[1].split(' ');
        return { pid, start: fields[19], state: fields[0] };
      };
      const sentinel = spawn(process.execPath, ['-e', 'setInterval(()=>{},1000)'], { stdio: 'ignore' });
      const sentinelDone = new Promise(resolve => sentinel.once('exit', resolve));
      const member = `process.on('SIGTERM',()=>{});console.log('ready');setInterval(()=>{},1000)`;
      const app = `const fs=require('fs');const id=pid=>({pid,start:fs.readFileSync('/proc/'+pid+'/stat','utf8').split(') ')[1].split(' ')[19]});process.on('SIGTERM',()=>fs.writeFileSync(${JSON.stringify(term)},'term'));${phase === 'after-status'
        ? `const child=require('child_process').spawn(process.execPath,['-e',${JSON.stringify(member)}],{stdio:['ignore','pipe','inherit']});child.stdout.once('data',()=>{fs.writeFileSync(${JSON.stringify(marker)},JSON.stringify([id(process.ppid),id(process.pid),id(child.pid)]));process.exit(23)});`
        : `fs.writeFileSync(${JSON.stringify(marker)},JSON.stringify([id(process.ppid),id(process.pid)]));setInterval(()=>{},1000);`}`;
      const child = spawn(process.execPath, ['-e', shim, 'machine-launch', token, process.execPath, '-e', app], { stdio: ['ignore', 'pipe', 'pipe'] });
      child.stdout.resume();
      let stderr = '';
      child.stderr.on('data', data => stderr += data);
      const done = new Promise(resolve => child.once('exit', (code, signal) => resolve({ code, signal })));
      let ids = [];
      async function until(predicate) {
        for (let i = 0; i < 100; i++) { if (predicate()) return; await pause(10); }
        throw Error('fixture readiness timed out');
      }
      try {
        await until(() => fs.existsSync(marker));
        ids = JSON.parse(fs.readFileSync(marker));
        if (phase === 'after-stop') {
          assert.equal(control('stop', token).status, 0);
          await until(() => fs.existsSync(term));
        } else if (phase === 'after-status') {
          await until(() => { try { return identity(ids[1].pid).state === 'Z'; } catch (e) { if (e.code === 'ENOENT') return true; throw e; } });
          await pause(50);
        }
        assert.equal(identity(ids[0].pid).start, ids[0].start);
        process.kill(ids[0].pid, 'SIGKILL');
        let timer;
        const result = await Promise.race([done, new Promise(resolve => { timer = setTimeout(() => resolve('timeout'), 3500); })]);
        clearTimeout(timer);
        assert.notEqual(result, 'timeout', 'outer retained application pipes after guardian fault');
        assert.deepEqual(result, { code: 125, signal: null });
        assert.equal(identity(ids.at(-1).pid).start, ids.at(-1).start);
        assert.notEqual(identity(ids.at(-1).pid).state, 'Z');
        assert.equal(sentinel.exitCode, null);
        assert.equal(fs.existsSync(`/tmp/dungeon-owned-${token}/result`), false);
        assert.doesNotMatch(stderr, /Unhandled|ERR_IPC/);
      } finally {
        if (ids.length === 0 && fs.existsSync(marker)) ids = JSON.parse(fs.readFileSync(marker));
        for (const id of ids.reverse()) {
          try { if (identity(id.pid).start === id.start) process.kill(id.pid, 'SIGKILL'); }
          catch (e) { if (e.code !== 'ENOENT' && e.code !== 'ESRCH') throw e; }
        }
        if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL');
        await done;
        sentinel.kill('SIGKILL');
        await sentinelDone;
        fs.rmSync(`/tmp/dungeon-owned-${token}`, { recursive: true, force: true });
        fs.rmSync(root, { recursive: true, force: true });
      }
    });
  }
});
test('successful framed execution still drains both output tails and preserves status', { timeout: 6000 }, async () => {
  const token = randomBytes(32).toString('hex');
  const app = "process.stdout.write(Buffer.alloc(131072,255),()=>process.stderr.write('tail\\0',()=>process.exit(23)))";
  const run = spawnSync(process.execPath, ['-e', shim, 'machine-launch', token, process.execPath, '-e', app], { encoding: 'utf8', timeout: 5000 });
  try {
    assert.equal(run.status, 23, run.stderr);
    const frames = run.stdout.trim().split('\n').map(line => JSON.parse(line));
    const bytes = stream => Buffer.concat(frames.filter(f => f.stream === stream).map(f => Buffer.from(f.data, 'base64')));
    frames.forEach((frame, index) => assert.equal(frame.seq, index));
    assert.deepEqual(bytes('stdout'), Buffer.alloc(131072, 255));
    assert.deepEqual(bytes('stderr'), Buffer.from('tail\0'));
    const result = control('result', token);
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(JSON.parse(result.stdout), { token, code: 23, output_complete: true });
    assert.equal(control('remove', token).status, 0);
  } finally { fs.rmSync(`/tmp/dungeon-owned-${token}`, { recursive: true, force: true }); }
});
