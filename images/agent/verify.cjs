const assert = require('node:assert/strict');
const { execFileSync } = require('node:child_process');
const path = require('node:path');
const { dependencies } = require('./package.json');

for (const [command, name, prefix, suffix] of [
  ['opencode', '@opencode/cli', 'opencode v', ''],
  ['claude', '@anthropic-ai/claude-code', '', ' (Claude Code)'],
  ['codex', '@openai/codex', 'codex-cli ', ''],
]) {
  const binary = process.argv.includes('--path')
    ? command
    : path.join(__dirname, 'node_modules', '.bin', command);
  const actual = execFileSync(binary, ['--version'], {
    encoding: 'utf8',
    timeout: 30000,
  }).trim();
  assert.equal(actual, `${prefix}${dependencies[name]}${suffix}`);
  console.log(actual);
}
