import { describe, expect, it } from "vitest";

import en from "./locales/en";
import zhCN from "./locales/zh-CN";

function keys(value: unknown, prefix = ""): string[] {
  if (!value || typeof value !== "object" || Array.isArray(value)) return [prefix];
  return Object.entries(value).flatMap(([key, child]) =>
    keys(child, prefix ? `${prefix}.${key}` : key),
  );
}

describe("localized messages", () => {
  it("keeps English and Simplified Chinese message shapes aligned", () => {
    expect(keys(zhCN).sort()).toEqual(keys(en).sort());
  });
});
