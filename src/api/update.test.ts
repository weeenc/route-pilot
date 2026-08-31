import { describe, expect, it, vi } from "vitest";

import { checkForUpdate, isVersionNewer } from "./update";

describe("isVersionNewer", () => {
  it("compares numeric version segments", () => {
    expect(isVersionNewer("v0.1.10", "0.1.2")).toBe(true);
    expect(isVersionNewer("1.0.0", "0.9.99")).toBe(true);
    expect(isVersionNewer("0.1.2", "0.1.2")).toBe(false);
    expect(isVersionNewer("0.1.1", "0.1.2")).toBe(false);
  });

  it("uses semantic-version prerelease precedence", () => {
    expect(isVersionNewer("1.0.0", "1.0.0-rc.1")).toBe(true);
    expect(isVersionNewer("1.0.0-rc.2", "1.0.0-rc.1")).toBe(true);
    expect(isVersionNewer("1.0.0-beta", "1.0.0")).toBe(false);
  });

  it("rejects tags that are not semantic versions", () => {
    expect(isVersionNewer("latest", "0.1.2")).toBe(false);
  });
});

describe("checkForUpdate", () => {
  it("returns release metadata when GitHub has a newer release", async () => {
    const loader = vi.fn().mockResolvedValue({
      version: "0.2.0",
      name: "RoutePilot 0.2.0",
      url: "https://github.com/weeenc/route-pilot/releases/tag/v0.2.0",
    });

    await expect(checkForUpdate("0.1.2", loader)).resolves.toEqual({
      version: "0.2.0",
      name: "RoutePilot 0.2.0",
      url: "https://github.com/weeenc/route-pilot/releases/tag/v0.2.0",
    });
  });

  it("returns null when the installed version is current", async () => {
    const loader = vi.fn().mockResolvedValue({
      version: "0.1.2",
      name: "RoutePilot v0.1.2",
      url: "https://github.com/weeenc/route-pilot/releases/tag/v0.1.2",
    });

    await expect(checkForUpdate("0.1.2", loader)).resolves.toBeNull();
  });

  it("rejects release links outside the project repository", async () => {
    const loader = vi.fn().mockResolvedValue({
      version: "9.0.0",
      name: "Untrusted release",
      url: "https://example.com/download",
    });

    await expect(checkForUpdate("0.1.2", loader)).rejects.toThrow(
      "valid URL",
    );
  });
});
