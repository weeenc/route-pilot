import type { AppErrorPayload } from "../types/api";
import { translate } from "../i18n";

const localizedErrorCodes = new Set([
  "PROFILE_NOT_FOUND",
  "CONFIG_INVALID",
  "OPENVPN_NOT_FOUND",
  "OPENVPN_INVALID_EXECUTABLE",
  "OPENVPN_START_FAILED",
  "OPENVPN_STOP_FAILED",
  "CONNECTION_ALREADY_ACTIVE",
  "MANAGEMENT_CONNECT_FAILED",
  "MANAGEMENT_TIMEOUT",
  "MANAGEMENT_PROTOCOL_INVALID",
  "AUTHENTICATION_FAILED",
  "PERMISSION_DENIED",
  "PRIVILEGED_HELPER_UNAVAILABLE",
  "PRIVILEGED_HELPER_INSTALL_FAILED",
  "ROUTE_CONFLICT",
  "PROFILE_STORE_CORRUPTED",
  "SETTINGS_CORRUPTED",
  "IO_ERROR",
  "UNSUPPORTED",
]);

export function normalizeAppError(error: unknown): AppErrorPayload {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error &&
    typeof error.code === "string" &&
    typeof error.message === "string"
  ) {
    return {
      code: error.code,
      message: localizedErrorCodes.has(error.code)
        ? translate(`errors.codes.${error.code}`)
        : error.message,
      details: "details" in error && typeof error.details === "string" ? error.details : null,
    };
  }

  return {
    code: "UNKNOWN_ERROR",
    message: error instanceof Error ? error.message : translate("errors.unexpected"),
    details: null,
  };
}
