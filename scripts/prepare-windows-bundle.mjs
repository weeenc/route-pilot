import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  mkdirSync,
  openSync,
  closeSync,
  readSync,
  realpathSync,
  statSync,
} from "node:fs";
import { dirname, extname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const scriptsDirectory = dirname(fileURLToPath(import.meta.url));

export const windowsBundleInput = resolve(
  scriptsDirectory,
  "../src-tauri/resources/openvpn-runtime.msi",
);

function normalizedSha256(value) {
  const checksum = value.trim().toLowerCase();
  if (!/^[a-f0-9]{64}$/.test(checksum)) {
    throw new Error("ROUTEPILOT_OPENVPN_MSI_SHA256 must contain exactly 64 hexadecimal characters");
  }
  return checksum;
}

function sha256(path) {
  const hash = createHash("sha256");
  const descriptor = openSync(path, "r");
  const buffer = Buffer.allocUnsafe(1024 * 1024);

  try {
    let bytesRead;
    do {
      bytesRead = readSync(descriptor, buffer, 0, buffer.length, null);
      hash.update(buffer.subarray(0, bytesRead));
    } while (bytesRead > 0);
  } finally {
    closeSync(descriptor);
  }

  return hash.digest("hex");
}

export function verifyAuthenticode(
  path,
  { spawn = spawnSync, parentEnvironment = process.env } = {},
) {
  const script = [
    "$signature = Get-AuthenticodeSignature -LiteralPath $env:ROUTEPILOT_SIGNATURE_INPUT",
    "if ($signature.Status -ne 'Valid') {",
    "  Write-Error \"OpenVPN MSI Authenticode signature is $($signature.Status)\"",
    "  exit 1",
    "}",
    "Write-Output $signature.SignerCertificate.Subject",
  ].join("; ");
  const environment = { ...parentEnvironment, ROUTEPILOT_SIGNATURE_INPUT: path };
  for (const key of Object.keys(environment)) {
    if (key.toLowerCase() === "psmodulepath") {
      delete environment[key];
    }
  }

  const failures = [];
  for (const executable of ["pwsh.exe", "powershell.exe"]) {
    const result = spawn(
      executable,
      ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script],
      { encoding: "utf8", env: environment },
    );

    if (result.error?.code === "ENOENT") {
      continue;
    }
    if (result.error) {
      failures.push(`${executable}: ${result.error.message}`);
      continue;
    }
    if (result.status === 0) {
      return result.stdout.trim();
    }

    failures.push(
      `${executable}: ${result.stderr.trim() || "the OpenVPN MSI signature is not valid"}`,
    );
  }

  throw new Error(
    failures.length > 0
      ? failures.join("\n")
      : "Unable to verify the OpenVPN MSI signature because PowerShell was not found",
  );
}

export function prepareWindowsBundle({
  source = process.env.ROUTEPILOT_OPENVPN_MSI,
  expectedSha256 = process.env.ROUTEPILOT_OPENVPN_MSI_SHA256,
  destination = windowsBundleInput,
  verifySignature = process.platform === "win32",
} = {}) {
  if (!source) {
    throw new Error(
      "ROUTEPILOT_OPENVPN_MSI must point to an official OpenVPN Community MSI",
    );
  }
  if (!expectedSha256) {
    throw new Error(
      "ROUTEPILOT_OPENVPN_MSI_SHA256 must pin the selected OpenVPN installer",
    );
  }

  const sourcePath = realpathSync(resolve(source));
  const metadata = statSync(sourcePath);
  if (!metadata.isFile() || extname(sourcePath).toLowerCase() !== ".msi") {
    throw new Error("ROUTEPILOT_OPENVPN_MSI must be a regular .msi file");
  }

  const expected = normalizedSha256(expectedSha256);
  const actual = sha256(sourcePath);
  if (actual !== expected) {
    throw new Error(`OpenVPN MSI checksum mismatch: expected ${expected}, received ${actual}`);
  }

  const signer = verifySignature ? verifyAuthenticode(sourcePath) : null;

  const destinationPath = resolve(destination);
  mkdirSync(dirname(destinationPath), { recursive: true });
  if (sourcePath !== destinationPath) {
    copyFileSync(sourcePath, destinationPath);
  }

  return { destination: destinationPath, sha256: actual, signer };
}

const invokedPath = process.argv[1] ? pathToFileURL(resolve(process.argv[1])).href : "";
if (import.meta.url === invokedPath) {
  const result = prepareWindowsBundle();
  console.log(`Prepared pinned OpenVPN runtime ${result.sha256} at ${result.destination}`);
  console.log(`Verified Authenticode signer: ${result.signer}`);
}
