import { describe, expect, it } from "vitest";

import type { PrivilegedHelperState, PrivilegedHelperStatus } from "../types/settings";
import { shouldShowPrivilegedHelperSettings } from "./platformSettings";

function helperStatus(state: PrivilegedHelperState): PrivilegedHelperStatus {
  return {
    state,
    installedVersion: null,
    expectedVersion: 5,
  };
}

describe("platform settings visibility", () => {
  it("hides the privileged helper only on unsupported platforms", () => {
    expect(shouldShowPrivilegedHelperSettings(helperStatus("unsupported"))).toBe(false);

    for (const state of [
      "installed",
      "notInstalled",
      "unavailable",
      "outdated",
    ] as const) {
      expect(shouldShowPrivilegedHelperSettings(helperStatus(state))).toBe(true);
    }
  });

  it("keeps the section visible while status is loading or unavailable", () => {
    expect(shouldShowPrivilegedHelperSettings(null)).toBe(true);
  });
});
