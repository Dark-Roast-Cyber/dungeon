import { readFileSync } from "node:fs";

// Build-time distribution metadata only; this is not part of the Dungeon runtime.
try {
  const cargo = readFileSync("Cargo.toml", "utf8");
  const version = cargo.match(/^version = "(\d+\.\d+\.\d+)"$/m)?.[1];
  if (!version) throw new Error("Cargo.toml must declare a stable MAJOR.MINOR.PATCH version");
  const [tag, ...extra] = process.argv.slice(2);
  if (extra.length || (tag !== undefined && tag !== `v${version}`)) {
    throw new Error(`tag must be v${version}`);
  }

  const locked = readFileSync("Cargo.lock", "utf8")
    .match(/^\[\[package\]\]\nname = "agent-dungeon"\nversion = "([^"]+)"/m)?.[1];
  if (locked !== version) throw new Error(`Cargo.lock: ${locked} != ${version}`);

  const launcher = JSON.parse(readFileSync("npm/launcher/package.json", "utf8"));
  if (launcher.version !== version) {
    throw new Error(`npm/launcher/package.json: ${launcher.version} != ${version}`);
  }
  for (const target of ["linux-x64", "darwin-arm64", "darwin-x64", "win32-x64"]) {
    const file = `npm/platform/${target}/package.json`;
    const manifest = JSON.parse(readFileSync(file, "utf8"));
    if (manifest.version !== version) throw new Error(`${file}: ${manifest.version} != ${version}`);
    const dependency = `@agent-archaeology/dungeon-${target}`;
    const requested = launcher.optionalDependencies?.[dependency];
    if (requested !== version) {
      throw new Error(`optional dependency ${dependency}: ${requested} != ${version}`);
    }
  }

  const changelog = readFileSync("CHANGELOG.md", "utf8");
  const section = changelog.split(/^## /m).find((part) => part.startsWith(`${version}\n`));
  const notes = section?.slice(version.length + 1).trim();
  if (!notes) throw new Error(`CHANGELOG.md: ${version} notes missing or empty`);
  process.stdout.write(`${notes}\n`);
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
