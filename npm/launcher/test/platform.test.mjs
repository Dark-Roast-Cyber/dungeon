import assert from "node:assert/strict";
import test from "node:test";
import { binaryName, platformPackage } from "../lib/platform.js";

test("supported platform packages are explicit", () => {
  assert.equal(platformPackage("linux", "x64"), "@agent-archaeology/dungeon-linux-x64");
  assert.equal(platformPackage("darwin", "arm64"), "@agent-archaeology/dungeon-darwin-arm64");
  assert.equal(platformPackage("darwin", "x64"), "@agent-archaeology/dungeon-darwin-x64");
  assert.equal(platformPackage("win32", "x64"), "@agent-archaeology/dungeon-win32-x64");
});
test("unsupported targets fail explicitly", () => {
  assert.throws(() => platformPackage("linux", "arm64"), /does not publish/);
});
test("windows binary keeps exe suffix", () => {
  assert.equal(binaryName("win32"), "dungeon.exe");
  assert.equal(binaryName("linux"), "dungeon");
});
