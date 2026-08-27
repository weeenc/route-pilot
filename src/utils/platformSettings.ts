import type { PrivilegedHelperStatus } from "../types/settings";

export function shouldShowPrivilegedHelperSettings(
  status: PrivilegedHelperStatus | null,
): boolean {
  return status?.state !== "unsupported";
}
