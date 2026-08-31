import { invoke, isTauri } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

import { appVersion } from "../version";

const latestReleaseApiUrl =
  "https://api.github.com/repos/weeenc/route-pilot/releases/latest";
const releaseUrlPrefix = "/weeenc/route-pilot/releases/";
const requestTimeoutMs = 8_000;

interface ParsedVersion {
  core: [number, number, number];
  prerelease: Array<number | string> | null;
}

interface GitHubReleaseResponse {
  tag_name?: unknown;
  html_url?: unknown;
  name?: unknown;
}

export interface ReleaseInfo {
  version: string;
  name: string;
  url: string;
}

type ReleaseLoader = () => Promise<ReleaseInfo>;

function parseVersion(value: string): ParsedVersion | null {
  const match = value
    .trim()
    .match(
      /^[vV]?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/,
    );
  if (!match) return null;

  const core = match.slice(1, 4).map(Number) as [number, number, number];
  if (!core.every(Number.isSafeInteger)) return null;

  const prerelease = match[4]
    ? match[4].split(".").map((identifier) => {
        const numeric = Number(identifier);
        return /^\d+$/.test(identifier) && Number.isSafeInteger(numeric)
          ? numeric
          : identifier;
      })
    : null;

  return { core, prerelease };
}

function comparePrerelease(
  candidate: ParsedVersion["prerelease"],
  current: ParsedVersion["prerelease"],
): number {
  if (candidate === null && current === null) return 0;
  if (candidate === null) return 1;
  if (current === null) return -1;

  const length = Math.max(candidate.length, current.length);
  for (let index = 0; index < length; index += 1) {
    const left = candidate[index];
    const right = current[index];
    if (left === undefined) return -1;
    if (right === undefined) return 1;
    if (left === right) continue;
    if (typeof left === "number" && typeof right === "string") return -1;
    if (typeof left === "string" && typeof right === "number") return 1;
    return left > right ? 1 : -1;
  }

  return 0;
}

export function isVersionNewer(candidate: string, current: string): boolean {
  const parsedCandidate = parseVersion(candidate);
  const parsedCurrent = parseVersion(current);
  if (!parsedCandidate || !parsedCurrent) return false;

  for (let index = 0; index < parsedCandidate.core.length; index += 1) {
    if (parsedCandidate.core[index] === parsedCurrent.core[index]) continue;
    return parsedCandidate.core[index] > parsedCurrent.core[index];
  }

  return comparePrerelease(
    parsedCandidate.prerelease,
    parsedCurrent.prerelease,
  ) > 0;
}

function validatedReleaseUrl(value: unknown): string | null {
  if (typeof value !== "string") return null;
  try {
    const url = new URL(value);
    return url.protocol === "https:" &&
      url.hostname === "github.com" &&
      url.pathname.startsWith(releaseUrlPrefix)
      ? url.toString()
      : null;
  } catch {
    return null;
  }
}

async function fetchLatestRelease(): Promise<ReleaseInfo> {
  const controller = new AbortController();
  const timeout = globalThis.setTimeout(() => controller.abort(), requestTimeoutMs);

  try {
    const response = await fetch(latestReleaseApiUrl, {
      headers: { Accept: "application/vnd.github+json" },
      signal: controller.signal,
    });
    if (!response.ok) {
      throw new Error(`GitHub Releases returned HTTP ${response.status}`);
    }

    const release = (await response.json()) as GitHubReleaseResponse;
    if (typeof release.tag_name !== "string") {
      throw new Error("The latest GitHub release does not have a valid tag");
    }

    const url = validatedReleaseUrl(release.html_url);
    if (!url) {
      throw new Error("The latest GitHub release does not have a valid URL");
    }

    const version = release.tag_name.replace(/^[vV]/, "");
    return {
      version,
      name:
        typeof release.name === "string" && release.name.trim()
          ? release.name.trim()
          : `RoutePilot v${version}`,
      url,
    };
  } finally {
    globalThis.clearTimeout(timeout);
  }
}

async function loadLatestRelease(): Promise<ReleaseInfo> {
  return isTauri()
    ? invoke<ReleaseInfo>("get_latest_release")
    : fetchLatestRelease();
}

export async function checkForUpdate(
  currentVersion = appVersion,
  loader: ReleaseLoader = loadLatestRelease,
): Promise<ReleaseInfo | null> {
  const release = await loader();
  const url = validatedReleaseUrl(release.url);
  if (!url) throw new Error("The latest GitHub release does not have a valid URL");
  if (!isVersionNewer(release.version, currentVersion)) return null;
  return { ...release, url };
}

export async function openReleasePage(url: string): Promise<void> {
  const validatedUrl = validatedReleaseUrl(url);
  if (!validatedUrl) throw new Error("Refusing to open an invalid release URL");

  if (isTauri()) {
    await openUrl(validatedUrl);
    return;
  }

  window.open(validatedUrl, "_blank", "noopener,noreferrer");
}
