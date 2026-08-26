import { describe, expect, it } from "vitest";

import { formatBytes, formatDuration } from "./format";

describe("formatBytes", () => {
  it("formats finite byte counts without changing units prematurely", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1023)).toBe("1023 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(Number.NaN)).toBe("0 B");
  });
});

describe("formatDuration", () => {
  it("formats elapsed time and handles invalid timestamps", () => {
    const start = "2026-08-26T00:00:00.000Z";
    expect(formatDuration(start, Date.parse(start) + 3_661_000)).toBe("01:01:01");
    expect(formatDuration(null, Date.now())).toBe("--:--:--");
    expect(formatDuration("invalid", Date.now())).toBe("--:--:--");
  });
});
