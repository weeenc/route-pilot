import { readFile } from "node:fs/promises";

const packageMetadata = JSON.parse(await readFile(new URL("../package.json", import.meta.url)));
const cargoManifest = await readFile(new URL("../src-tauri/Cargo.toml", import.meta.url), "utf8");
const cargoVersion = cargoManifest.match(/^version\s*=\s*"([^"]+)"/m)?.[1];

if (!cargoVersion) {
  throw new Error("Could not read the RoutePilot version from src-tauri/Cargo.toml");
}
if (packageMetadata.version !== cargoVersion) {
  throw new Error(
    `Version mismatch: package.json=${packageMetadata.version}, Cargo.toml=${cargoVersion}`,
  );
}

console.log(`RoutePilot version ${packageMetadata.version} is consistent.`);
