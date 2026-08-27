import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import test from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";

const scriptsDirectory = dirname(fileURLToPath(import.meta.url));
const hook = readFileSync(
  resolve(scriptsDirectory, "../src-tauri/windows/nsis-hooks.nsh"),
  "utf8",
);

test("reuses a standard OpenVPN installation with a network driver", () => {
  const existingRuntimeCheck = hook.indexOf(
    '${FileExists} "$PROGRAMFILES64\\OpenVPN\\bin\\openvpn.exe"',
  );
  const existingDriverCheck = hook.indexOf(
    'ReadRegStr $1 HKLM "SYSTEM\\CurrentControlSet\\Services\\ovpn-dco"',
  );
  const installCommand = hook.indexOf('ExecWait \'"$SYSDIR\\msiexec.exe"');

  assert.notEqual(existingRuntimeCheck, -1);
  assert.notEqual(existingDriverCheck, -1);
  assert.notEqual(installCommand, -1);
  assert.ok(existingRuntimeCheck < installCommand);
  assert.ok(existingDriverCheck < installCommand);
  assert.match(hook, /Using the existing OpenVPN runtime/);
  assert.match(hook, /Goto routepilot_openvpn_ready/);
});

test("checks both standard installation directories", () => {
  assert.match(hook, /\$PROGRAMFILES64\\OpenVPN\\bin\\openvpn\.exe/);
  assert.match(hook, /\$PROGRAMFILES32\\OpenVPN\\bin\\openvpn\.exe/);
});

test("requires an OpenVPN executable and network driver after installation", () => {
  assert.match(hook, /The OpenVPN runtime was not found after installation/);
  assert.match(hook, /An OpenVPN network driver was not found after installation/);
  assert.match(hook, /Services\\tap0901/);
});
