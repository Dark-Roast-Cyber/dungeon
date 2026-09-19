import { cpSync, mkdirSync, readFileSync, writeFileSync, chmodSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import process from "node:process";

const targets = {
  "linux-x64": "dungeon",
  "darwin-arm64": "dungeon",
  "darwin-x64": "dungeon",
  "win32-x64": "dungeon.exe",
};

function packageVersion() {
  return JSON.parse(readFileSync(new URL("./launcher/package.json", import.meta.url), "utf8")).version;
}

export function stagePackages(nativeRoot, outputRoot) {
  const version = packageVersion();
  mkdirSync(outputRoot, { recursive: true });

  for (const [target, binary] of Object.entries(targets)) {
    const template = new URL(`./platform/${target}/package.json`, import.meta.url);
    const manifest = JSON.parse(readFileSync(template, "utf8"));
    if (manifest.version !== version) {
      throw new Error(`${target} package version ${manifest.version} does not match launcher ${version}`);
    }

    const sourceBinary = join(nativeRoot, target, binary);
    const packageDir = join(outputRoot, target);
    const binDir = join(packageDir, "bin");
    mkdirSync(binDir, { recursive: true });

    delete manifest.private;
    writeFileSync(join(packageDir, "package.json"), JSON.stringify(manifest, null, 2) + "\n");
    cpSync(sourceBinary, join(binDir, binary));
    cpSync(resolve("LICENSE"), join(packageDir, "LICENSE"));
    cpSync(resolve("README.md"), join(packageDir, "README.md"));

    if (target !== "win32-x64") {
      chmodSync(join(binDir, binary), 0o755);
    }
  }

  return { version, targets: Object.keys(targets) };
}

if (process.argv[1] && basename(process.argv[1]) === "stage-packages.mjs") {
  const [nativeRoot, outputRoot] = process.argv.slice(2);
  if (!nativeRoot || !outputRoot) {
    console.error("usage: node npm/stage-packages.mjs NATIVE_ROOT OUTPUT_ROOT");
    process.exit(2);
  }

  const result = stagePackages(resolve(nativeRoot), resolve(outputRoot));
  console.log(`staged ${result.targets.length} platform packages for ${result.version}`);
}
