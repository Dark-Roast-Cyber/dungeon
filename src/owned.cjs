'use strict';

// This fixed program runs only in the guest. Command arguments are never code.
const fs = require('node:fs');
const { spawn } = require('node:child_process');
const C = fs.constants;
const [mode, token, ...argv] = process.argv.slice(1);
if (process.platform !== 'linux' || !/^[a-f0-9]{64}$/.test(token || '')) process.exit(125);
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
    if (fs.fstatSync(fd).size > 128) throw Error('oversized result');
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
    command = spawn(argv[0], argv.slice(1), { stdio: ['ignore', 'inherit', 'inherit'], detached: false });
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

try {
  if (mode === 'launch') {
    if (!argv[0]) throw Error('missing command');
    // mkdir is exclusive: never adopt existing control state.
    fs.mkdirSync(directory, { mode: 0o700 });
    const base = openDirectory();
    const control = fs.openSync(`${base}/stop`, C.O_RDWR | C.O_CREAT | C.O_EXCL | flags, 0o600);
    const child = spawn(process.execPath, ['-e', guardian, '--', ...argv], {
      detached: true, stdio: ['ignore', 'inherit', 'inherit', 'ipc'],
    });
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
        if (ready && child.connected) child.send('stop');
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
          child.send(stopped ? 'stop' : 'start');
        } catch { child.disconnect(); }
      } else if (Number.isInteger(value?.code) && value.code >= 0 && value.code <= 255) code = value.code;
    });
    child.once('error', () => { clearInterval(interval); process.exitCode = 125; });
    child.once('exit', (_status, signal) => {
      clearInterval(interval);
      fs.closeSync(control);
      // SIGKILL alone does not prove a group kill: OOM/external kills can leave work.
      if (signal !== 'SIGKILL' || (!stopped && code === undefined)) {
        process.exitCode = 125;
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
            const result = fs.openSync(`${base}/result`, C.O_WRONLY | C.O_CREAT | C.O_EXCL | flags, 0o600);
            fs.writeFileSync(result, JSON.stringify({ token, code: code ?? 143 }));
            fs.closeSync(result);
            process.exitCode = code ?? 143;
            return;
          }
          process.exitCode = 125;
          return;
        }
        if (Date.now() >= deadline) {
          process.stderr.write('owned group disappearance unconfirmed\n');
          process.exitCode = 125;
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
