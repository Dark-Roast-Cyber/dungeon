import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

const command = fileURLToPath(new URL("./check-release.mjs", import.meta.url));

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "dungeon-release-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  function write(path, content) {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), typeof content === "string" ? content : JSON.stringify(content));
  }
  write("Cargo.toml", '[package]\nname = "agent-dungeon"\nversion = "0.3.0"\n');
  write("Cargo.lock", 'version = 4\n\n[[package]]\nname = "agent-dungeon"\nversion = "0.3.0"\n');
  write("CHANGELOG.md", "# Changelog\n\n## 0.3.0\n\n- First Rust preview.\n\n## 0.0.1\n\nPython preview.\n");
  const dependencies = {
    "@agent-archaeology/dungeon-linux-x64": "0.3.0",
    "@agent-archaeology/dungeon-darwin-arm64": "0.3.0",
    "@agent-archaeology/dungeon-darwin-x64": "0.3.0",
    "@agent-archaeology/dungeon-win32-x64": "0.3.0",
  };
  write("npm/launcher/package.json", { version: "0.3.0", optionalDependencies: dependencies });
  write("npm/platform/linux-x64/package.json", { version: "0.3.0" });
  write("npm/platform/darwin-arm64/package.json", { version: "0.3.0" });
  write("npm/platform/darwin-x64/package.json", { version: "0.3.0" });
  write("npm/platform/win32-x64/package.json", { version: "0.3.0" });
  return { root, write, dependencies };
}

function check(root, ...args) {
  return spawnSync(process.execPath, [command, ...args], { cwd: root, encoding: "utf8" });
}

test("matching release metadata emits only the selected release notes", (t) => {
  const { root } = fixture(t);
  const result = check(root, "v0.3.0");
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, "- First Rust preview.\n");
});

test("a tag for another version cannot publish this release", (t) => {
  const { root } = fixture(t);
  const result = check(root, "v0.3.1");
  assert.equal(result.status, 1);
  assert.match(result.stderr, /tag must be v0\.3\.0/);
  assert.equal(result.stdout, "");
});

test("a stale lockfile cannot pass release checks", (t) => {
  const { root, write } = fixture(t);
  write("Cargo.lock", 'version = 4\n\n[[package]]\nname = "agent-dungeon"\nversion = "0.0.2"\n');
  const result = check(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Cargo\.lock.*0\.0\.2.*0\.3\.0/);
});

test("a stale platform package cannot pass release checks", (t) => {
  const { root, write } = fixture(t);
  write("npm/platform/linux-x64/package.json", { version: "0.0.2" });
  const result = check(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /linux-x64\/package\.json.*0\.0\.2.*0\.3\.0/);
});

test("launcher dependencies must resolve to the release being packaged", (t) => {
  const { root, write, dependencies } = fixture(t);
  dependencies["@agent-archaeology/dungeon-linux-x64"] = "0.0.2";
  write("npm/launcher/package.json", { version: "0.3.0", optionalDependencies: dependencies });
  const result = check(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /optional dependency.*dungeon-linux-x64.*0\.0\.2.*0\.3\.0/);
});

test("missing curated release notes block publication", (t) => {
  const { root, write } = fixture(t);
  write("CHANGELOG.md", "# Changelog\n\n## Unreleased\n\n- Not reviewed yet.\n");
  const result = check(root, "v0.3.0");
  assert.equal(result.status, 1);
  assert.match(result.stderr, /CHANGELOG\.md.*0\.3\.0.*missing or empty/);
});
