export function platformPackage(platform = process.platform, arch = process.arch) {
  const packages = {
    "linux-x64": "@agent-archaeology/dungeon-linux-x64",
    "darwin-arm64": "@agent-archaeology/dungeon-darwin-arm64",
    "darwin-x64": "@agent-archaeology/dungeon-darwin-x64",
    "win32-x64": "@agent-archaeology/dungeon-win32-x64"
  };
  const packageName = packages[`${platform}-${arch}`];
  if (!packageName) throw new Error(`Dungeon does not publish an npm binary for ${platform}-${arch}`);
  return packageName;
}
export function binaryName(platform = process.platform) {
  return platform === "win32" ? "dungeon.exe" : "dungeon";
}
