import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { stagePackages } from "../stage-packages.mjs";

test("staging removes private only from generated platform packages", () => {
  const root = mkdtempSync(join(tmpdir(), "dungeon-npm-"));
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
  assert.equal(result.version, "0.0.2");
  assert.equal(result.targets.length, 4);

  const generated = JSON.parse(readFileSync(join(staged, "linux-x64", "package.json"), "utf8"));
  assert.equal(generated.private, undefined);
  assert.equal(generated.version, "0.0.2");

  const template = JSON.parse(
    readFileSync(new URL("../platform/linux-x64/package.json", import.meta.url), "utf8")
  );
  assert.equal(template.private, true);
});
