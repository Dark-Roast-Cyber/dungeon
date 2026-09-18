#!/usr/bin/env node
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { spawnSync } from "node:child_process";
import { binaryName, platformPackage } from "../lib/platform.js";

const require = createRequire(import.meta.url);
let packageJson;
try {
  packageJson = require.resolve(`${platformPackage()}/package.json`);
} catch {
  console.error(`dungeon: native package is unavailable for ${process.platform}-${process.arch}`);
  process.exit(1);
}
const binary = join(dirname(packageJson), "bin", binaryName());
const result = spawnSync(binary, process.argv.slice(2), {stdio: "inherit", shell: false, windowsHide: false});
if (result.error) {
  console.error(`dungeon: failed to start native binary: ${result.error.message}`);
  process.exit(1);
}
if (result.signal) {
  console.error(`dungeon: native binary exited from signal ${result.signal}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
