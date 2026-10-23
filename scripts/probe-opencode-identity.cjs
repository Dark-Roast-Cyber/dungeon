// Run only inside a disposable, network-disabled reviewed image (see image README).
// This probes native harness compatibility, not OpenShell substitution or OAuth.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

const marker = 'openshell:resolve:env:v1_DUNGEON_SYNTHETIC_KEY';
const proof = 'DUNGEON_SYNTHETIC_MODEL_PROOF';
let stage = 'disposable-guard';

function assertNoPersistedMarker(root) {
  let scanned = 0, entries = 0;
  function scan(directory, depth) {
    assert(depth <= 32, 'synthetic tree exceeds depth bound');
    for (const name of fs.readdirSync(directory)) {
      assert(++entries <= 8192, 'synthetic tree exceeds entry bound');
      if (name === '.env' || name.startsWith('.env.') || name.endsWith('.env')) continue;
      const filename = path.join(directory, name);
      const stat = fs.lstatSync(filename);
      assert(!stat.isSymbolicLink(), 'synthetic tree unexpectedly contains a link');
      if (stat.isDirectory()) scan(filename, depth + 1);
      else if (stat.isFile()) {
        scanned += stat.size;
        assert(stat.size <= 536870912 && scanned <= 1073741824, 'synthetic tree scan exceeds byte bound');
        const fd = fs.openSync(filename, 'r');
        try {
          const buffer = Buffer.alloc(1048576);
          let tail = Buffer.alloc(0), offset = 0;
          while (offset < stat.size) {
            const count = fs.readSync(fd, buffer, 0, Math.min(buffer.length, stat.size - offset), offset);
            assert(count > 0, 'synthetic tree file changed during scan');
            const data = Buffer.concat([tail, buffer.subarray(0, count)]);
            assert(!data.includes(Buffer.from(marker)), 'native harness persisted the marker');
            tail = Buffer.from(data.subarray(Math.max(0, data.length - marker.length + 1)));
            offset += count;
          }
        } finally { fs.closeSync(fd); }
      }
    }
  }
  scan(root, 0);
}

function run(args, env, cwd) {
  return new Promise((resolve, reject) => {
    const child = spawn('opencode', args, { env, cwd, stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '', bytes = 0, failed = false;
    const timeout = setTimeout(() => {
      failed = true;
      child.kill('SIGKILL');
    }, 45000);
    child.stdout.on('data', chunk => {
      bytes += chunk.length;
      if (bytes > 1048576) {
        failed = true;
        child.kill('SIGKILL');
      } else stdout += chunk.toString();
    });
    child.stderr.on('data', chunk => {
      bytes += chunk.length;
      if (bytes > 1048576) {
        failed = true;
        child.kill('SIGKILL');
      }
    });
    child.once('error', error => {
      clearTimeout(timeout);
      reject(error);
    });
    child.once('exit', code => {
      clearTimeout(timeout);
      resolve({ code, stdout, bounded: !failed });
    });
  });
}

async function probe() {
  assert.equal(process.env.DUNGEON_SYNTHETIC_PROBE, '1', 'explicit disposable probe opt-in required');
  assert.equal(process.env.HOME, '/sandbox', 'run in the reviewed disposable image, not on the host');
  assert.equal(process.getuid(), 998, 'run as the image sandbox user');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dungeon-identity-proof-'));
  fs.mkdirSync(path.join(root, 'tmp'), { mode: 0o700 });
  const requests = [];
  const server = http.createServer((request, response) => {
    let size = 0;
    request.on('data', chunk => {
      size += chunk.length;
      if (size > 1048576) request.destroy();
    });
    request.on('end', () => {
      requests.push({ path: request.url, authorization: request.headers.authorization });
      if (request.url !== '/v1/chat/completions' || request.headers.authorization !== `Bearer ${marker}`) {
        response.writeHead(401).end();
        return;
      }
      response.writeHead(200, { 'content-type': 'text/event-stream' });
      const base = { id: 'synthetic', object: 'chat.completion.chunk', created: 1, model: 'proof' };
      for (const chunk of [
        { ...base, choices: [{ index: 0, delta: { role: 'assistant', content: proof }, finish_reason: null }] },
        { ...base, choices: [{ index: 0, delta: {}, finish_reason: 'stop' }], usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 } },
      ]) response.write(`data: ${JSON.stringify(chunk)}\n\n`);
      response.end('data: [DONE]\n\n');
    });
  });
  try {
    stage = 'loopback-fixture';
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    const baseURL = `http://127.0.0.1:${server.address().port}/v1`;
    const rounds = [];
    let versionChecked = false;
    for (const project of ['a', 'b', 'a']) {
      const home = path.join(root, `home-${project}`);
      const cwd = path.join(root, `project-${project}`);
      const config = path.join(home, '.config/opencode');
      fs.mkdirSync(cwd, { recursive: true, mode: 0o700 });
      fs.mkdirSync(config, { recursive: true, mode: 0o700 });
      fs.writeFileSync(path.join(config, 'opencode.json'), JSON.stringify({
        update: 'disable', share: 'disabled', snapshots: false,
        providers: {
          'dungeon-proof': {
            name: 'Synthetic identity compatibility probe',
            env: ['DUNGEON_SYNTHETIC_KEY'],
            package: '@opencode/ai/providers/openai-compatible',
            settings: { baseURL },
            models: { proof: { limit: { context: 8192, output: 1024 } } },
          },
        },
      }), { mode: 0o600 });
      const env = {
        HOME: home, PATH: '/usr/local/bin:/usr/bin:/bin', LANG: 'C.UTF-8',
        TMPDIR: path.join(root, 'tmp'),
        XDG_CONFIG_HOME: path.join(home, '.config'),
        XDG_DATA_HOME: path.join(home, '.local/share'),
        XDG_STATE_HOME: path.join(home, '.local/state'),
        XDG_CACHE_HOME: path.join(home, '.cache'),
        DUNGEON_SYNTHETIC_KEY: marker,
      };
      const before = requests.length;
      if (!versionChecked) {
        stage = 'reviewed-version';
        const version = await run(['--version'], env, cwd);
        assert(version.bounded && version.code === 0 && version.stdout.trim() === 'opencode v2.1.2', 'probe requires reviewed OpenCode2.1.2');
        versionChecked = true;
      }
      stage = 'native-model-call';
      const result = await run(['run', '--standalone', '--model', 'dungeon-proof/proof', '--format', 'json', 'Return the synthetic proof. Do not use tools.'], env, cwd);
      assert(result.bounded, 'native harness probe exceeded time/output bound');
      assert.equal(result.code, 0, 'native model request failed');
      assert(result.stdout.includes(proof), 'native harness did not emit the synthetic model response');
      assert(requests.length > before, 'no native model request reached the loopback fixture');
      assert(requests.slice(before).every(r => r.authorization === `Bearer ${marker}`), 'credential marker was not preserved');
      stage = 'environment-discovery';
      const auth = await run(['auth', 'list', '--standalone', '--format', 'json'], env, cwd);
      assert(auth.bounded && auth.code === 0, 'native authentication inventory failed');
      const inventory = JSON.parse(auth.stdout);
      assert(inventory.some(item => item.id === 'dungeon-proof' && item.connections.some(c => c.type === 'env')), 'environment credential was not discovered');
      if (project === 'b') {
        stage = 'opted-out-run';
        const optedOut = { ...env };
        delete optedOut.DUNGEON_SYNTHETIC_KEY;
        const disconnected = await run(['auth', 'list', '--standalone', '--format', 'json'], optedOut, cwd);
        assert(disconnected.bounded && disconnected.code === 0, 'opted-out inventory failed');
        assert(!JSON.parse(disconnected.stdout).some(item => item.id === 'dungeon-proof'), 'opted-out environment retained a connection');
        const start = requests.length;
        const denied = await run(['run', '--standalone', '--model', 'dungeon-proof/proof', '--format', 'json', 'Return the synthetic proof. Do not use tools.'], optedOut, cwd);
        assert(denied.bounded && !denied.stdout.includes(proof), 'opted-out run obtained a model proof');
        assert(requests.slice(start).every(r => r.authorization !== `Bearer ${marker}`), 'opted-out run replayed the marker');
      }
      rounds.push({ project, native_model_call: true, environment_connection: true });
    }
    stage = 'synthetic-persistence-scan';
    assertNoPersistedMarker(root);
    return {
      gate: 'native_opencode_environment_marker', rounds,
      distinct_guest_state_roots: 2,
      pinned_opencode: '2.1.2',
      opted_out_environment_has_no_connection: true,
      opted_out_run_cannot_replay_marker: true,
      synthetic_home_project_tmp_do_not_persist_marker: true,
      upstream: 'loopback synthetic SSE fixture; no real provider',
      credential_material: 'synthetic marker only; no refresh token',
      openshell_substitution: 'NOT_TESTED',
      oauth_enrollment_refresh_revocation: 'NOT_TESTED',
      shared_signin_acceptance: 'INCOMPLETE',
    };
  } finally {
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
    fs.rmSync(root, { recursive: true, force: true });
  }
}

probe().then(report => console.log(JSON.stringify(report)), error => {
  // Never emit native output, request bodies or environment contents on failure.
  const fixedErrors = [
    'synthetic tree exceeds depth bound', 'synthetic tree exceeds entry bound',
    'synthetic tree unexpectedly contains a link', 'synthetic tree scan exceeds byte bound',
    'native harness persisted the marker',
  ];
  const code = fixedErrors.includes(error.message) ? error.message : stage;
  console.error(`Synthetic native identity compatibility probe failed (${code}); shared sign-in is not established.`);
  process.exitCode = 1;
});
