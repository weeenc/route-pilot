import type { VpnRoute } from "../types/vpn";

export interface RouteSummary {
  visibleRoutes: VpnRoute[];
  hiddenCount: number;
}

export function summarizeRoutes(routes: VpnRoute[], limit = 6): RouteSummary {
  const uniqueRoutes = Array.from(
    new Map(routes.map((route) => [route.network, route])).values(),
  ).sort((left, right) =>
    left.network.localeCompare(right.network, undefined, { numeric: true }),
  );
  const visibleCount = Math.max(0, limit);

  return {
    visibleRoutes: uniqueRoutes.slice(0, visibleCount),
    hiddenCount: Math.max(0, uniqueRoutes.length - visibleCount),
  };
}
