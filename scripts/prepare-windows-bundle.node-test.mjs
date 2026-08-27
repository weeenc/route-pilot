import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import assert from "node:assert/strict";

import {
  prepareWindowsBundle,
  verifyAuthenticode,
} from "./prepare-windows-bundle.mjs";

function fixture() {
  const directory = mkdtempSync(join(tmpdir(), "routepilot-windows-bundle-"));
  const source = join(directory, "openvpn.msi");
  const destination = join(directory, "bundle-input", "openvpn-runtime.msi");
  const contents = Buffer.from("test OpenVPN MSI fixture");
  writeFileSync(source, contents);
  return {
    contents,
    destination,
    sha256: createHash("sha256").update(contents).digest("hex"),
    source,
  };
}

test("copies a checksum-pinned MSI into the bundle input directory", () => {
  const input = fixture();
  const result = prepareWindowsBundle({
    source: input.source,
    expectedSha256: input.sha256.toUpperCase(),
    destination: input.destination,
    verifySignature: false,
  });

  assert.equal(result.destination, input.destination);
  assert.equal(result.sha256, input.sha256);
  assert.deepEqual(readFileSync(input.destination), input.contents);
});

test("rejects an MSI whose checksum does not match", () => {
  const input = fixture();

  assert.throws(
    () =>
      prepareWindowsBundle({
        source: input.source,
        expectedSha256: "0".repeat(64),
        destination: input.destination,
        verifySignature: false,
      }),
    /checksum mismatch/,
  );
});

test("prefers PowerShell 7 and falls back with a clean module path", () => {
  const calls = [];
  const missing = Object.assign(new Error("not found"), { code: "ENOENT" });
  const signer = verifyAuthenticode("C:\\runtime\\openvpn.msi", {
    parentEnvironment: { PATH: "C:\\Windows", PSModulePath: "incompatible" },
    spawn(executable, args, options) {
      calls.push({ executable, args, options });
      if (executable === "pwsh.exe") {
        return { error: missing };
      }
      return { status: 0, stderr: "", stdout: "CN=OpenVPN, Inc.\n" };
    },
  });

  assert.equal(signer, "CN=OpenVPN, Inc.");
  assert.deepEqual(
    calls.map(({ executable }) => executable),
    ["pwsh.exe", "powershell.exe"],
  );
  assert.equal(calls[1].options.env.PSModulePath, undefined);
  assert.equal(
    calls[1].options.env.ROUTEPILOT_SIGNATURE_INPUT,
    "C:\\runtime\\openvpn.msi",
  );
});
