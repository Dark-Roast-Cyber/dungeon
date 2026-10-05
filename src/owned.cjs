'use strict';

// This fixed program runs only in the guest. Command arguments are never code.
const fs = require('node:fs');
const { spawn } = require('node:child_process');
const C = fs.constants;
const [mode, token, ...argv] = process.argv.slice(1);
if (process.platform !== 'linux' || !/^[a-f0-9]{64}$/.test(token || '')) process.exit(125);
const framed = mode === 'machine-launch' || mode === 'stream-launch';
const unowned = mode === 'stream-launch';
const directory = `/tmp/dungeon-owned-${token}`;
const flags = C.O_NOFOLLOW | C.O_NONBLOCK;
let dir;
function openDirectory() {
  dir = fs.openSync(directory, C.O_RDONLY | C.O_DIRECTORY | C.O_NOFOLLOW);
  const stat = fs.fstatSync(dir);
  if (stat.uid !== process.getuid() || (stat.mode & 0o777) !== 0o700) throw Error('invalid control directory');
  return `/proc/self/fd/${dir}`;
}
function regular(fd) {
  const stat = fs.fstatSync(fd);
  if (!stat.isFile() || stat.uid !== process.getuid() || (stat.mode & 0o777) !== 0o600 || stat.nlink !== 1) throw Error('invalid control file');
}
function receipt(base) {
  const fd = fs.openSync(`${base}/result`, C.O_RDONLY | flags);
  try {
    regular(fd);
    if (fs.fstatSync(fd).size > 256) throw Error('oversized result');
    const value = JSON.parse(fs.readFileSync(fd, 'utf8'));
    if (value.token !== token || !Number.isInteger(value.code) || value.code < 0 || value.code > 255) throw Error('invalid result');
    return value;
  } finally { fs.closeSync(fd); }
}

const guardian = String.raw`
'use strict';
const fs = require('node:fs');
const { spawn } = require('node:child_process');
const argv = process.argv.slice(1);
const framed = argv.shift() === 'framed';
// detached spawn establishes a new session; validate it before starting work.
const stat = fs.readFileSync('/proc/self/stat', 'utf8');
const fields = stat.slice(stat.lastIndexOf(')') + 2).split(' ');
if (Number(fields[2]) !== process.pid || Number(fields[3]) !== process.pid) process.exit(125);
let finishing = false;
let command;
function finish() {
  if (finishing) return;
  finishing = true;
  // Caught (not ignored) signals are reset by exec: the workload gets normal TERM.
  process.kill(-process.pid, 'SIGTERM');
  setTimeout(() => process.kill(-process.pid, 'SIGKILL'), 1000);
}
process.on('SIGTERM', finish);
process.on('SIGINT', finish);
process.on('disconnect', finish);
process.on('message', value => {
  if (value === 'stop') finish();
  if (value === 'start' && !finishing && !command) {
    command = spawn(argv[0], argv.slice(1), { stdio: framed ? ['ignore', 4, 5] : ['ignore', 'inherit', 'inherit'], detached: false });
    command.once('error', () => {
      process.send({ code: 127 }, finish);
    });
    command.once('exit', (code, signal) => {
      const numbers = require('node:os').constants.signals;
      process.send({ code: code === null ? 128 + (numbers[signal] || 0) : code }, finish);
    });
  }
});
process.send('ready');
`;

function ttyReceipt(base) {
  const fd = fs.openSync(`${base}/result`, C.O_RDONLY | flags);
  try {
    regular(fd);
    if (fs.fstatSync(fd).size > 512) throw Error('oversized result');
    // Host strict JSON parsing is authoritative, including duplicate-key refusal.
    const text = fs.readFileSync(fd, 'utf8');
    const value = JSON.parse(text);
    if (value.token !== token || value.clean !== true) throw Error('invalid result');
    return text;
  } finally { fs.closeSync(fd); }
}

function ttyLaunch() {
  if (!argv[0]) throw Error('missing command');
  const stat = fs.readFileSync('/proc/self/stat', 'utf8');
  const fields = stat.slice(stat.lastIndexOf(')') + 2).split(' ');
  if (+fields[2] !== process.pid || +fields[3] !== process.pid || +fields[4] === 0 ||
      +fields[5] !== process.pid || !process.stdin.isTTY || !process.stdout.isTTY) throw Error('foreground terminal required');
  process.chdir('/sandbox/project');
  // The observer has no terminal descriptors and never signals a guest group.
  const self = fs.readFileSync('/proc/self/cmdline', 'utf8').split('\0')[2];
  let observer, command, finishing = false, requested = false, fault = false, status = null, revision = 0;
  let timer;
  function send(value) {
    if (observer?.connected) observer.send(value, error => { if (error) kill(); });
    else kill();
  }
  function kill() {
    if (finishing) return;
    finishing = true;
    clearTimeout(timer);
    process.kill(-process.pid, 'SIGTERM');
    setTimeout(() => process.kill(-process.pid, 'SIGKILL'), 1000);
  }
  function finish(broken = false) {
    const changed = broken && !fault;
    fault ||= broken;
    if (finishing || (requested && !changed)) return;
    requested = true;
    clearTimeout(timer);
    timer = setTimeout(() => {
      fault = true;
      revision++;
      if (observer?.connected) observer.send({ finish: true, revision, fault, status }, () => kill());
      setTimeout(kill, 100);
    }, 2000);
    revision++;
    send({ finish: true, revision, fault, status });
  }
  // Catch, not ignore: exec resets caught dispositions in the application.
  process.on('SIGINT', () => {});
  process.on('SIGTERM', () => { if (!finishing) finish(true); });
  process.on('SIGHUP', () => { if (!finishing) finish(true); });
  observer = spawn(process.execPath, ['-e', self, '--', 'tty-observe', token, String(process.pid)], {
    detached: true, stdio: ['ignore', 'ignore', 'ignore', 'ipc'],
  });
  timer = setTimeout(() => finish(true), 2000);
  observer.once('error', () => { fault = true; kill(); });
  observer.on('disconnect', () => { fault = true; kill(); });
  observer.on('message', value => {
    if (value?.finish_ack === revision && requested) { kill(); return; }
    if (value === 'stop') { finish(); return; }
    if (value !== 'start' || requested || finishing || command) return;
    clearTimeout(timer);
    send('started');
    command = spawn(argv[0], argv.slice(1), { detached: false, stdio: ['inherit', 'inherit', 'inherit'] });
    command.once('error', () => { status = { code: null, signal: null, spawn_error: true }; finish(); });
    command.once('exit', (code, signal) => { status = { code, signal, spawn_error: false }; finish(); });
  });
}

function ttyObserve() {
  const group = Number(argv[0]);
  if (!Number.isSafeInteger(group) || group < 1) throw Error('invalid guardian');
  fs.mkdirSync(directory, { mode: 0o700 });
  const base = openDirectory();
  const control = fs.openSync(`${base}/stop`, C.O_RDWR | C.O_CREAT | C.O_EXCL | flags, 0o600);
  let stopped = false, fault = false, finish = false, status = null, disconnected = false;
  function send(value) {
    if (process.connected) process.send(value, error => { if (error) disconnect(); });
  }
  function disconnect() {
    fault = true;
    if (process.connected) process.disconnect();
  }
  function check() {
    regular(control);
    const size = fs.fstatSync(control).size;
    if (size > 1) throw Error('invalid stop request');
    if (size === 1) {
      const byte = Buffer.alloc(1);
      fs.readSync(control, byte, 0, 1, 0);
      if (byte[0] !== 1) throw Error('invalid stop request');
      if (!stopped) { stopped = true; send('stop'); }
    }
  }
  const startup = setTimeout(disconnect, 2000);
  const interval = setInterval(() => { try { check(); } catch { disconnect(); } }, 20);
  process.on('message', value => {
    if (value === 'started') clearTimeout(startup);
    else if (value?.finish === true && Number.isSafeInteger(value.revision) && value.revision > 0) {
      clearTimeout(startup);
      finish = true;
      fault ||= value.fault !== false;
      status = value.status;
      send({ finish_ack: value.revision });
    }
  });
  process.on('disconnect', () => {
    if (disconnected) return;
    disconnected = true;
    clearTimeout(startup);
    clearInterval(interval);
    fs.closeSync(control);
    fault ||= !finish;
    const deadline = Date.now() + 2000;
    function confirm() {
      try { process.kill(-group, 0); }
      catch (error) {
        if (error.code !== 'ESRCH') { process.exitCode = 125; return; }
        const fd = fs.openSync(`${base}/result`, C.O_WRONLY | C.O_CREAT | C.O_EXCL | flags, 0o600);
        fs.writeFileSync(fd, JSON.stringify({ token, clean: true, stopped, fault, status }));
        fs.closeSync(fd);
        return;
      }
      if (Date.now() >= deadline) process.exitCode = 125;
      else setTimeout(confirm, 20);
    }
    confirm();
  });
  check();
  if (!stopped) send('start');
}

try {
  if (mode === 'tty-launch') ttyLaunch();
  else if (mode === 'tty-observe') ttyObserve();
  else if (mode === 'tty-ready') {
    const base = openDirectory();
    const fd = fs.openSync(`${base}/stop`, C.O_RDONLY | flags);
    try { regular(fd); process.stdout.write(JSON.stringify({ token })); } finally { fs.closeSync(fd); }
  } else if (mode === 'tty-result' || mode === 'tty-remove') {
    const base = openDirectory();
    const text = ttyReceipt(base);
    if (mode === 'tty-result') process.stdout.write(text);
    else {
      fs.unlinkSync(`${base}/result`);
      fs.unlinkSync(`${base}/stop`);
      fs.rmdirSync(directory);
    }
  } else if (mode === 'launch' || framed) {
    if (!argv[0]) throw Error('missing command');
    // mkdir is exclusive: never adopt existing control state.
    fs.mkdirSync(directory, { mode: 0o700 });
    const base = openDirectory();
    const control = fs.openSync(`${base}/stop`, C.O_RDWR | C.O_CREAT | C.O_EXCL | flags, 0o600);
    const child = unowned
      ? spawn(argv[0], argv.slice(1), { detached: false, stdio: ['pipe', 'pipe', 'pipe'] })
      : spawn(process.execPath, ['-e', guardian, '--', framed ? 'framed' : 'native', ...argv], {
        detached: true, stdio: framed ? ['ignore', 'ignore', 'ignore', 'ipc', 'pipe', 'pipe'] : ['ignore', 'inherit', 'inherit', 'ipc'],
      });
    if (unowned) {
      process.stdin.pipe(child.stdin);
      child.stdin.on('error', () => {});
    }
    let sequence = 0;
    let outputComplete = !framed;
    let outputFailed = false;
    function send(value) {
      if (child.connected) child.send(value, error => {
        // A closing IPC pipe is not a spawn failure. Disconnect asks the guardian
        // to finish; only its exit and kernel group absence can confirm cleanup.
        if (error && child.connected) child.disconnect();
      });
    }
    function writeFrame(value) {
      return new Promise((resolve, reject) => {
        process.stdout.write(JSON.stringify({ v: 1, seq: sequence++, ...value }) + '\n', error => error ? reject(error) : resolve());
      });
    }
    if (framed) {
      process.stdout.on('error', () => { outputFailed = true; send('stop'); });
      async function frame(stream, source) {
        for await (const chunk of source) {
          for (let offset = 0; offset < chunk.length; offset += 4096) {
            await writeFrame({ event: 'output', stream, data: chunk.subarray(offset, offset + 4096).toString('base64') });
          }
        }
      }
      Promise.all([frame('stdout', unowned ? child.stdout : child.stdio[4]), frame('stderr', unowned ? child.stderr : child.stdio[5])])
        .then(() => { outputComplete = true; }, () => { outputFailed = true; send('stop'); });
    }
    let code;
    let stopped = false;
    let ready = false;
    function checkStop() {
      regular(control);
      const size = fs.fstatSync(control).size;
      if (size > 1) throw Error('invalid stop request');
      if (size === 1) {
        const byte = Buffer.alloc(1);
        fs.readSync(control, byte, 0, 1, 0);
        if (byte[0] !== 1) throw Error('invalid stop request');
        stopped = true;
        if (ready) send('stop');
      }
    }
    const interval = setInterval(() => {
      try { checkStop(); } catch { if (child.connected) child.disconnect(); }
    }, 20);
    child.on('message', value => {
      if (value === 'ready') {
        ready = true;
        try {
          checkStop();
          send(stopped ? 'stop' : 'start');
        } catch { child.disconnect(); }
      } else if (Number.isInteger(value?.code) && value.code >= 0 && value.code <= 255) code = value.code;
    });
    child.once('error', () => {
      clearInterval(interval);
      if (unowned) { fs.closeSync(control); code = 127; finishOutput(); }
      else process.exit(125);
    });
    function finishOutput() {
      const deadline = Date.now() + 2000;
      function drain() {
        if (!outputComplete && !outputFailed && Date.now() < deadline) { setTimeout(drain, 20); return; }
        const clean = outputComplete && !outputFailed;
        const result = fs.openSync(`${base}/result`, C.O_WRONLY | C.O_CREAT | C.O_EXCL | flags, 0o600);
        fs.writeFileSync(result, JSON.stringify({ token, code: clean ? (code ?? 143) : 125, ...(framed ? { output_complete: clean } : {}) }));
        fs.closeSync(result);
        if (!clean) { process.exit(125); return; }
        process.exitCode = code ?? 143;
      }
      drain();
    }
    child.once('exit', (_status, signal) => {
      clearInterval(interval);
      fs.closeSync(control);
      if (unowned) {
        const numbers = require('node:os').constants.signals;
        code = _status === null ? 128 + (numbers[signal] || 0) : _status;
        finishOutput();
        return;
      }
      // SIGKILL alone does not prove a group kill: OOM/external kills can leave work.
      if (signal !== 'SIGKILL' || (!stopped && code === undefined)) {
        // Surviving workloads can retain framed pipes; faults must not await EOF.
        process.exit(125);
        return;
      }
      const deadline = Date.now() + 2000;
      function confirmAbsent() {
        try {
          // Signal zero is observation only. A surviving member reserves its PGID
          // even after its leader is reaped. Reuse can only cause conservative
          // refusal, never authorize signalling another group or hide survivors.
          process.kill(-child.pid, 0);
        } catch (error) {
          if (error.code === 'ESRCH') {
            finishOutput();
            return;
          }
          process.exit(125);
          return;
        }
        if (Date.now() >= deadline) {
          process.stderr.write('owned group disappearance unconfirmed\n');
          process.exit(125);
        } else setTimeout(confirmAbsent, 20);
      }
      confirmAbsent();
    });
  } else if (mode === 'stop') {
    const base = openDirectory();
    const fd = fs.openSync(`${base}/stop`, C.O_WRONLY | flags);
    try { regular(fd); fs.writeSync(fd, Buffer.from([1]), 0, 1, 0); } finally { fs.closeSync(fd); }
  } else if (mode === 'result' || mode === 'remove') {
    const base = openDirectory();
    const value = receipt(base);
    if (mode === 'result') process.stdout.write(JSON.stringify(value));
    else {
      // unlink never follows a substituted symlink, and rmdir is not recursive.
      fs.unlinkSync(`${base}/result`);
      fs.unlinkSync(`${base}/stop`);
      fs.rmdirSync(directory);
    }
  } else throw Error('unsupported operation');
} catch {
  process.stderr.write('owned guest control unavailable or invalid\n');
  process.exitCode = 125;
}
