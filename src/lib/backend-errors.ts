import type { TFunction } from "i18next";

/**
 * Backend error codes returned from Rust Tauri commands.
 * Keep this list in sync with the codes used in `src-tauri/src/profile/password.rs`.
 */
export type BackendErrorCode =
  | "INCORRECT_PASSWORD"
  | "LOCKED_OUT"
  | "PROFILE_NOT_FOUND"
  | "PROFILE_NOT_PROTECTED"
  | "PROFILE_ALREADY_PROTECTED"
  | "PROFILE_RUNNING"
  | "PROFILE_EPHEMERAL"
  | "PROFILE_MISSING_SALT"
  | "PROFILE_LOCKED"
  | "INVALID_PROFILE_ID"
  | "PASSWORD_TOO_SHORT"
  | "INVALID_LAUNCH_HOOK_URL"
  | "COOKIE_DB_LOCKED"
  | "COOKIE_DB_UNAVAILABLE"
  | "PROXY_NOT_FOUND"
  | "GROUP_NOT_FOUND"
  | "GROUP_ALREADY_EXISTS"
  | "NAME_CANNOT_BE_EMPTY"
  | "WAYFERN_VERSION_NOT_AVAILABLE"
  | "VPN_NOT_FOUND"
  | "EXTENSION_NOT_FOUND"
  | "EXTENSION_GROUP_NOT_FOUND"
  | "EXTENSION_UNSUPPORTED_FILE_TYPE"
  | "EXTENSION_DIR_NOT_FOUND"
  | "EXTENSION_NOT_A_DIRECTORY"
  | "EXTENSION_MANIFEST_MISSING"
  | "EXTENSION_MANIFEST_INVALID"
  | "EXTENSION_DIR_TOO_LARGE"
  | "EXTENSION_PATH_HAS_COMMA"
  | "EXTENSION_LINK_REQUIRES_DIRECTORY"
  | "EXTENSION_LINKED_CANNOT_SYNC"
  | "SYNC_LOCKED_BY_PROFILE"
  | "SYNC_NOT_CONFIGURED"
  | "PROXY_NOT_WORKING"
  | "PROXY_PAYMENT_REQUIRED"
  | "CANNOT_MODIFY_CLOUD_MANAGED_PROXY"
  | "VPN_NOT_WORKING"
  | "CAMOUFOX_IMPORT_DEPRECATED"
  | "PROXY_SIDECAR_VERSION_MISMATCH"
  | "UPDATE_CHECKSUMS_UNAVAILABLE"
  | "UPDATE_CHECKSUM_MISMATCH"
  | "UPDATE_PROFILES_RUNNING"
  | "UPDATE_PREPARATION_FAILED"
  | "PROFILE_NAME_EXISTS"
  | "IMPORT_SOURCE_NOT_FOUND"
  | "IMPORT_SOURCE_NOT_CHROMIUM"
  | "IMPORT_SOURCE_BROWSER_RUNNING"
  | "IMPORT_NO_ITEMS"
  | "BROWSER_NOT_DOWNLOADED"
  | "ARCHIVE_EXTRACTION_FAILED"
  | "UNSUPPORTED_ARCHIVE_FORMAT"
  | "CLEAR_ON_CLOSE_UNAVAILABLE"
  | "PROXY_AND_VPN_MUTUALLY_EXCLUSIVE"
  | "FINGERPRINT_MATCH_FAILED"
  | "INVALID_DNS_RULES_JSON"
  | "UNSUPPORTED_DNS_RULES_FORMAT"
  | "DNS_RULES_SAVE_FAILED"
  | "DNS_RULES_EXPORT_FAILED"
  | "WAYFERN_TERMS_REQUIRED"
  | "API_PORT_UNAVAILABLE"
  | "MCP_SERVER_ALREADY_RUNNING"
  | "MCP_SERVER_NOT_RUNNING"
  | "MCP_PORT_UNAVAILABLE"
  | "MCP_CONFIGURATION_UNAVAILABLE"
  | "MCP_AGENT_UNKNOWN"
  | "MCP_AGENT_INSTALL_FAILED"
  | "MCP_AGENT_REMOVE_FAILED"
  | "VLESS_CONFIG_INVALID"
  | "XRAY_UNAVAILABLE"
  | "XRAY_UNSUPPORTED_OS"
  | "XRAY_START_FAILED"
  | "FINGERPRINT_EXIT_MISMATCH"
  | "LAUNCH_CONSENT_EXPIRED"
  | "VPN_WORKER_START_FAILED"
  | "EXIT_PROBE_FAILED"
  | "CAMOUFOX_REMOVED"
  | "NO_E2E_PASSWORD_SET"
  | "INTERNAL_ERROR";

export interface BackendError {
  code: BackendErrorCode;
  params?: Record<string, string>;
}

/**
 * Try to parse a backend error string as a structured `{code, params}` payload.
 * Returns null if the string isn't structured (e.g. raw error from a command
 * that doesn't yet emit codes — caller should fall back to showing the raw text).
 */
export function parseBackendError(err: unknown): BackendError | null {
  const message = err instanceof Error ? err.message : String(err);
  if (!message.startsWith("{")) return null;
  try {
    const parsed = JSON.parse(message);
    if (
      parsed &&
      typeof parsed === "object" &&
      typeof parsed.code === "string"
    ) {
      return parsed as BackendError;
    }
  } catch {
    // not JSON
  }
  return null;
}

/**
 * Translate a backend error to a localized string. Falls back to the raw
 * message if the error isn't a structured backend error.
 */
export function translateBackendError(t: TFunction, err: unknown): string {
  const parsed = parseBackendError(err);
  if (!parsed) {
    return err instanceof Error ? err.message : String(err);
  }
  switch (parsed.code) {
    case "INCORRECT_PASSWORD":
      return t("backendErrors.incorrectPassword");
    case "LOCKED_OUT": {
      const seconds = Number.parseInt(parsed.params?.seconds ?? "0", 10);
      return t("backendErrors.lockedOut", {
        duration: formatLockoutDuration(t, seconds),
      });
    }
    case "PROFILE_NOT_FOUND":
      return t("backendErrors.profileNotFound");
    case "PROFILE_NOT_PROTECTED":
      return t("backendErrors.profileNotProtected");
    case "PROFILE_ALREADY_PROTECTED":
      return t("backendErrors.profileAlreadyProtected");
    case "PROFILE_RUNNING":
      return t("backendErrors.profileRunning");
    case "PROFILE_EPHEMERAL":
      return t("backendErrors.profileEphemeral");
    case "PROFILE_MISSING_SALT":
      return t("backendErrors.profileMissingSalt");
    case "PROFILE_LOCKED":
      return t("backendErrors.profileLocked");
    case "INVALID_PROFILE_ID":
      return t("backendErrors.invalidProfileId");
    case "PASSWORD_TOO_SHORT": {
      const min = Number.parseInt(parsed.params?.min ?? "8", 10);
      return t("backendErrors.passwordTooShort", { min });
    }
    case "INVALID_LAUNCH_HOOK_URL":
      return t("backendErrors.invalidLaunchHookUrl");
    case "COOKIE_DB_LOCKED":
      return t("backendErrors.cookieDbLocked");
    case "COOKIE_DB_UNAVAILABLE":
      return t("backendErrors.cookieDbUnavailable");
    case "PROXY_NOT_FOUND":
      return t("backendErrors.proxyNotFound");
    case "GROUP_NOT_FOUND":
      return t("backendErrors.groupNotFound");
    case "GROUP_ALREADY_EXISTS":
      return t("backendErrors.groupAlreadyExists");
    case "NAME_CANNOT_BE_EMPTY":
      return t("backendErrors.nameCannotBeEmpty");
    case "WAYFERN_VERSION_NOT_AVAILABLE":
      return t("backendErrors.wayfernVersionNotAvailable", {
        requested: parsed.params?.requested ?? "",
        current: parsed.params?.current ?? "",
      });
    case "VPN_NOT_FOUND":
      return t("backendErrors.vpnNotFound");
    case "EXTENSION_NOT_FOUND":
      return t("backendErrors.extensionNotFound");
    case "EXTENSION_GROUP_NOT_FOUND":
      return t("backendErrors.extensionGroupNotFound");
    case "EXTENSION_UNSUPPORTED_FILE_TYPE":
      return t("backendErrors.extensionUnsupportedFileType");
    case "EXTENSION_DIR_NOT_FOUND":
      return t("backendErrors.extensionDirNotFound");
    case "EXTENSION_NOT_A_DIRECTORY":
      return t("backendErrors.extensionNotADirectory");
    case "EXTENSION_MANIFEST_MISSING":
      return t("backendErrors.extensionManifestMissing");
    case "EXTENSION_MANIFEST_INVALID":
      return t("backendErrors.extensionManifestInvalid");
    case "EXTENSION_DIR_TOO_LARGE":
      return t("backendErrors.extensionDirTooLarge");
    case "EXTENSION_PATH_HAS_COMMA":
      return t("backendErrors.extensionPathHasComma");
    case "EXTENSION_LINK_REQUIRES_DIRECTORY":
      return t("backendErrors.extensionLinkRequiresDirectory");
    case "EXTENSION_LINKED_CANNOT_SYNC":
      return t("backendErrors.extensionLinkedCannotSync");
    case "SYNC_LOCKED_BY_PROFILE":
      return t("backendErrors.syncLockedByProfile");
    case "SYNC_NOT_CONFIGURED":
      return t("backendErrors.syncNotConfigured");
    case "PROXY_NOT_WORKING":
      return t("backendErrors.proxyNotWorking");
    case "PROXY_PAYMENT_REQUIRED":
      return t("backendErrors.proxyPaymentRequired");
    case "CANNOT_MODIFY_CLOUD_MANAGED_PROXY":
      return t("backendErrors.cannotModifyCloudManagedProxy");
    case "VPN_NOT_WORKING":
      return t("backendErrors.vpnNotWorking");
    case "CAMOUFOX_IMPORT_DEPRECATED":
      return t("backendErrors.camoufoxImportDeprecated");
    case "PROXY_SIDECAR_VERSION_MISMATCH":
      return t("backendErrors.proxySidecarVersionMismatch");
    case "UPDATE_CHECKSUMS_UNAVAILABLE":
      return t("backendErrors.updateChecksumsUnavailable", {
        version: parsed.params?.version ?? "",
      });
    case "UPDATE_CHECKSUM_MISMATCH":
      return t("backendErrors.updateChecksumMismatch", {
        file: parsed.params?.file ?? "",
      });
    case "UPDATE_PROFILES_RUNNING":
      return t("backendErrors.updateProfilesRunning");
    case "UPDATE_PREPARATION_FAILED":
      return t("backendErrors.updatePreparationFailed");
    case "PROFILE_NAME_EXISTS":
      return t("backendErrors.profileNameExists", {
        name: parsed.params?.name ?? "",
      });
    case "IMPORT_SOURCE_NOT_FOUND":
      return t("backendErrors.importSourceNotFound");
    case "IMPORT_SOURCE_NOT_CHROMIUM":
      return parsed.params?.family
        ? t("backendErrors.importSourceNotChromiumNamed", {
            family: parsed.params.family,
          })
        : t("backendErrors.importSourceNotChromium");
    case "IMPORT_SOURCE_BROWSER_RUNNING":
      return t("backendErrors.importSourceBrowserRunning", {
        browser: parsed.params?.browser ?? "",
      });
    case "IMPORT_NO_ITEMS":
      return t("backendErrors.importNoItems");
    case "BROWSER_NOT_DOWNLOADED":
      return t("backendErrors.browserNotDownloaded", {
        browser: parsed.params?.browser ?? "",
      });
    case "ARCHIVE_EXTRACTION_FAILED":
      return t("backendErrors.archiveExtractionFailed", {
        detail: parsed.params?.detail ?? "",
      });
    case "UNSUPPORTED_ARCHIVE_FORMAT":
      return t("backendErrors.unsupportedArchiveFormat");
    case "PROXY_AND_VPN_MUTUALLY_EXCLUSIVE":
      return t("backendErrors.proxyAndVpnMutuallyExclusive");
    case "FINGERPRINT_MATCH_FAILED":
      return t("backendErrors.fingerprintMatchFailed");
    case "INVALID_DNS_RULES_JSON":
      return t("backendErrors.invalidDnsRulesJson");
    case "UNSUPPORTED_DNS_RULES_FORMAT":
      return t("backendErrors.unsupportedDnsRulesFormat", {
        format: parsed.params?.format ?? "",
      });
    case "DNS_RULES_SAVE_FAILED":
      return t("backendErrors.dnsRulesSaveFailed");
    case "DNS_RULES_EXPORT_FAILED":
      return t("backendErrors.dnsRulesExportFailed");
    case "WAYFERN_TERMS_REQUIRED":
      return t("backendErrors.wayfernTermsRequired");
    case "API_PORT_UNAVAILABLE":
      return t("backendErrors.apiPortUnavailable");
    case "MCP_SERVER_ALREADY_RUNNING":
      return t("backendErrors.mcpServerAlreadyRunning");
    case "MCP_SERVER_NOT_RUNNING":
      return t("backendErrors.mcpServerNotRunning");
    case "MCP_PORT_UNAVAILABLE":
      return t("backendErrors.mcpPortUnavailable");
    case "MCP_CONFIGURATION_UNAVAILABLE":
      return t("backendErrors.mcpConfigurationUnavailable");
    case "MCP_AGENT_UNKNOWN":
      return t("backendErrors.mcpAgentUnknown");
    case "MCP_AGENT_INSTALL_FAILED":
      return t("backendErrors.mcpAgentInstallFailed", {
        detail: parsed.params?.detail ?? "",
      });
    case "MCP_AGENT_REMOVE_FAILED":
      return t("backendErrors.mcpAgentRemoveFailed", {
        detail: parsed.params?.detail ?? "",
      });
    // Donut supports exactly one VLESS shape (REALITY + XTLS Vision over TCP),
    // so most rejections mean "your server is a kind we do not support", not
    // "you mistyped". Name the unsupported part instead of implying a typo.
    case "VLESS_CONFIG_INVALID": {
      const reason = parsed.params?.reason;
      const known = [
        "security",
        "flow",
        "transport",
        "encryption",
        "headerType",
        "fingerprint",
        "sni",
        "publicKey",
        "scheme",
        "parameter",
        "malformed",
      ];
      if (reason && known.includes(reason)) {
        return t(`backendErrors.vlessUnsupported.${reason}`);
      }
      return t("backendErrors.vlessConfigInvalid");
    }
    case "XRAY_UNAVAILABLE":
      return t("backendErrors.xrayUnavailable");
    case "XRAY_UNSUPPORTED_OS":
      return t("backendErrors.xrayUnsupportedOs");
    case "XRAY_START_FAILED":
      return t("backendErrors.xrayStartFailed");
    case "CLEAR_ON_CLOSE_UNAVAILABLE":
      return t("backendErrors.clearOnCloseUnavailable");
    // The launch gate's block. The dialog renders the mismatch detail from
    // `params` itself; this string is the fallback for anywhere that only has
    // room for one sentence.
    case "FINGERPRINT_EXIT_MISMATCH":
      return t("backendErrors.fingerprintExitMismatch");
    case "LAUNCH_CONSENT_EXPIRED":
      return t("backendErrors.launchConsentExpired");
    case "VPN_WORKER_START_FAILED":
      return t("backendErrors.vpnWorkerStartFailed", {
        detail: parsed.params?.detail ?? "",
      });
    case "EXIT_PROBE_FAILED":
      return t("backendErrors.exitProbeFailed");
    case "CAMOUFOX_REMOVED":
      return t("backendErrors.camoufoxRemoved");
    case "NO_E2E_PASSWORD_SET":
      return t("backendErrors.noE2ePasswordSet");
    case "INTERNAL_ERROR":
      return t("backendErrors.internal", {
        detail: parsed.params?.detail ?? "",
      });
    default:
      // The payload parsed as a structured error but carries a code this build
      // does not know: the server can add codes faster than the desktop ships.
      // Returning the raw message here would render the literal JSON to the
      // user, so show a translated line that still names the code for support.
      return t("backendErrors.unknownCode", { code: String(parsed.code) });
  }
}

export function formatLockoutDuration(t: TFunction, seconds: number): string {
  if (seconds < 60)
    return t("backendErrors.lockedOutDuration.seconds", { seconds });
  const minutes = Math.ceil(seconds / 60);
  if (minutes < 60)
    return t("backendErrors.lockedOutDuration.minutes", { minutes });
  const hours = Math.ceil(minutes / 60);
  return t("backendErrors.lockedOutDuration.hours", { hours });
}

/**
 * Extract the lockout countdown in seconds from a backend error, or null.
 */
export function extractLockoutSeconds(err: unknown): number | null {
  const parsed = parseBackendError(err);
  if (parsed?.code !== "LOCKED_OUT") return null;
  const secs = Number.parseInt(parsed.params?.seconds ?? "0", 10);
  return Number.isFinite(secs) && secs > 0 ? secs : null;
}

/**
 * True if the error is a known structured backend error code.
 */
export function isBackendErrorCode(
  err: unknown,
  code: BackendErrorCode,
): boolean {
  return parseBackendError(err)?.code === code;
}
