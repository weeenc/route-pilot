import { describe, expect, it } from "vitest";

import type { VpnRoute } from "../types/vpn";
import { summarizeRoutes } from "./routes";

function route(network: string, source: VpnRoute["source"] = "serverPush"): VpnRoute {
  return { network, source, gateway: null, interface: null };
}

describe("summarizeRoutes", () => {
  it("sorts and deduplicates route networks before applying the display limit", () => {
    const summary = summarizeRoutes(
      [
        route("192.168.10.0/24"),
        route("10.2.192.0/22"),
        route("10.0.0.0/8"),
        route("10.0.0.0/8", "config"),
      ],
      2,
    );

    expect(summary.visibleRoutes.map(({ network }) => network)).toEqual([
      "10.0.0.0/8",
      "10.2.192.0/22",
    ]);
    expect(summary.hiddenCount).toBe(1);
  });

  it("handles an empty route list and a negative display limit", () => {
    expect(summarizeRoutes([])).toEqual({ visibleRoutes: [], hiddenCount: 0 });
    expect(summarizeRoutes([route("10.0.0.0/8")], -1)).toEqual({
      visibleRoutes: [],
      hiddenCount: 1,
    });
  });
});
