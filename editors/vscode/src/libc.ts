import * as fs from "node:fs";

/** The C library of a Linux host, as the release asset target names it. */
export type LinuxLibc = "gnu" | "musl";

const ALPINE_RELEASE_FILE = "/etc/alpine-release";

/** The host probes that libc detection reads. Tests replace them. */
export type LibcProbes = {
  /** Returns true when the file exists. */
  readonly fileExists: (filePath: string) => boolean;
  /**
   * Returns the glibc version from the Node diagnostic report header, an empty
   * value when the report has no glibc version, or throws when no report is
   * available.
   */
  readonly glibcVersionRuntime: () => string | null;
};

type ReportHeader = { readonly glibcVersionRuntime?: unknown };

// Node 20.13 and later accept `excludeNetwork`. `@types/node` does not declare it yet.
type ProcessReportWithNetworkSwitch = NodeJS.ProcessReport & { excludeNetwork?: boolean };

const readReportHeader = (report: unknown): ReportHeader | null => {
  if (typeof report !== "object" || report === null || !("header" in report)) {
    return null;
  }
  const { header } = report;
  return typeof header === "object" && header !== null ? header : null;
};

/**
 * Read `header.glibcVersionRuntime` from the Node diagnostic report. A Node
 * runtime that links glibc sets this field. A musl runtime does not set it.
 * The report skips the network section, because that section does reverse DNS
 * lookups that can block the extension host.
 */
const readGlibcVersionRuntime = (): string | null => {
  const report: ProcessReportWithNetworkSwitch | undefined = process.report;
  if (!report) {
    throw new Error("the diagnostic report is not available");
  }
  // Older Node runtimes have no switch. The setter rejects a non-boolean value.
  const previousExcludeNetwork = report.excludeNetwork;
  const hasNetworkSwitch = typeof previousExcludeNetwork === "boolean";
  if (hasNetworkSwitch) {
    report.excludeNetwork = true;
  }
  try {
    const header = readReportHeader(report.getReport());
    if (!header) {
      throw new Error("the diagnostic report has no header");
    }
    const version = header.glibcVersionRuntime;
    return typeof version === "string" && version.length > 0 ? version : null;
  } finally {
    if (hasNetworkSwitch) {
      report.excludeNetwork = previousExcludeNetwork;
    }
  }
};

const HOST_PROBES: LibcProbes = {
  fileExists: (filePath) => fs.existsSync(filePath),
  glibcVersionRuntime: readGlibcVersionRuntime,
};

/**
 * Detect the C library of a Linux host.
 *
 * Alpine is musl. On other hosts the Node diagnostic report decides: a glibc
 * version in the report header means glibc, no glibc version means musl. When
 * no report is available, the result is glibc, which was the only Linux target
 * before musl detection existed.
 */
export const detectLinuxLibc = (probes: LibcProbes = HOST_PROBES): LinuxLibc => {
  if (probes.fileExists(ALPINE_RELEASE_FILE)) {
    return "musl";
  }
  try {
    return probes.glibcVersionRuntime() ? "gnu" : "musl";
  } catch {
    return "gnu";
  }
};

let hostLibc: LinuxLibc | null = null;

/** The C library of this Linux host. The first call detects it, later calls reuse the result. */
export const hostLinuxLibc = (): LinuxLibc => {
  hostLibc ??= detectLinuxLibc();
  return hostLibc;
};
