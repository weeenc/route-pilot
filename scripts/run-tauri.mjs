import { spawnSync } from "node:child_process";
import { delimiter } from "node:path";

const environment = { ...process.env };

// Tauri's macOS bundler relies on Apple's xattr implementation. Python
// distributions can install an incompatible executable earlier in PATH.
if (process.platform === "darwin") {
  const systemPaths = ["/usr/bin", "/bin", "/usr/sbin", "/sbin"];
  environment.PATH = [...systemPaths, environment.PATH ?? ""]
    .filter(Boolean)
    .join(delimiter);
}

const executable = process.platform === "win32" ? "tauri.cmd" : "tauri";
const result = spawnSync(executable, process.argv.slice(2), {
  env: environment,
  stdio: "inherit",
});

if (result.error) {
  console.error(`Unable to start Tauri CLI: ${result.error.message}`);
  process.exit(1);
}

process.exit(result.status ?? 1);
