import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";
import { stagePackages } from "../stage-packages.mjs";

test("staging removes private only from generated platform packages", (t) => {
  const root = mkdtempSync(join(tmpdir(), "dungeon-npm-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const native = join(root, "native");
  const staged = join(root, "staged");

  for (const [target, binary] of [
    ["linux-x64", "dungeon"],
    ["darwin-arm64", "dungeon"],
    ["darwin-x64", "dungeon"],
    ["win32-x64", "dungeon.exe"],
  ]) {
    mkdirSync(join(native, target), { recursive: true });
    writeFileSync(join(native, target, binary), "synthetic binary");
  }

  const result = stagePackages(native, staged);
  assert.equal(result.version, "0.3.0");
  assert.equal(result.targets.length, 4);

  const generated = JSON.parse(readFileSync(join(staged, "linux-x64", "package.json"), "utf8"));
  assert.equal(generated.private, undefined);
  assert.equal(generated.version, "0.3.0");

  const template = JSON.parse(
    readFileSync(new URL("../platform/linux-x64/package.json", import.meta.url), "utf8")
  );
  assert.equal(template.private, true);
});

test("packed Linux launcher preserves executable bytes, literal argv and native exit status", { skip: process.platform !== "linux" }, (t) => {
  const root = mkdtempSync(join(tmpdir(), "dungeon-pack-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const native = join(root, "native");
  mkdirSync(join(native, "linux-x64"), { recursive: true });
  mkdirSync(join(native, "darwin-arm64"), { recursive: true });
  mkdirSync(join(native, "darwin-x64"), { recursive: true });
  mkdirSync(join(native, "win32-x64"), { recursive: true });
  writeFileSync(join(native, "linux-x64/dungeon"), '#!/bin/sh\nprintf "%s\\n" "$1"\nexit 23\n');
  writeFileSync(join(native, "darwin-arm64/dungeon"), "synthetic, not distributable");
  writeFileSync(join(native, "darwin-x64/dungeon"), "synthetic, not distributable");
  writeFileSync(join(native, "win32-x64/dungeon.exe"), "synthetic, not distributable");
  const staged = join(root, "staged");
  stagePackages(native, staged);
  const platformPack = spawnSync("npm", ["pack", "--pack-destination", root], {
    cwd: join(staged, "linux-x64"), encoding: "utf8",
  });
  assert.equal(platformPack.status, 0, platformPack.stderr);
  const launcherPack = spawnSync("npm", ["pack", "--pack-destination", root], {
    cwd: new URL("../launcher", import.meta.url), encoding: "utf8",
  });
  assert.equal(launcherPack.status, 0, launcherPack.stderr);
  const platform = join(root, "node_modules/@agent-archaeology/dungeon-linux-x64");
  const launcher = join(root, "node_modules/@agent-archaeology/dungeon");
  mkdirSync(platform, { recursive: true });
  mkdirSync(launcher, { recursive: true });
  const platformExtract = spawnSync("tar", ["-xzf", join(root, "agent-archaeology-dungeon-linux-x64-0.3.0.tgz"), "--strip-components=1", "-C", platform]);
  assert.equal(platformExtract.status, 0);
  const launcherExtract = spawnSync("tar", ["-xzf", join(root, "agent-archaeology-dungeon-0.3.0.tgz"), "--strip-components=1", "-C", launcher]);
  assert.equal(launcherExtract.status, 0);
  const result = spawnSync(process.execPath, [join(launcher, "bin/dungeon.js"), "two words;$(literal)"], { encoding: "utf8" });
  assert.equal(result.status, 23, result.stderr);
  assert.equal(result.stdout, "two words;$(literal)\n");
});
