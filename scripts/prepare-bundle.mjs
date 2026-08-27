import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

const scriptsDirectory = dirname(fileURLToPath(import.meta.url));
const projectDirectory = resolve(scriptsDirectory, "..");

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: projectDirectory,
    env: process.env,
    stdio: "inherit",
  });

  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

switch (process.platform) {
  case "darwin":
    run("/bin/sh", [join(scriptsDirectory, "prepare-macos-bundle.sh")]);
    break;
  case "win32":
    run(process.execPath, [join(scriptsDirectory, "prepare-windows-bundle.mjs")]);
    break;
  default:
    throw new Error(`RoutePilot cannot prepare a bundle for ${process.platform}`);
}
