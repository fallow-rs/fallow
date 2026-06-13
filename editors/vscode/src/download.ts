import { execFileSync } from "node:child_process";
import { createHash, createPublicKey, verify } from "node:crypto";
import * as fs from "node:fs";
import type { IncomingMessage } from "node:http";
import * as https from "node:https";
import * as os from "node:os";
import * as path from "node:path";
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import { getExecutableExtension } from "./binary-utils.js";

const GITHUB_REPO = "fallow-rs/fallow";
const LSP_BINARY_NAME = "fallow-lsp";
const CLI_BINARY_NAME = "fallow";
const VERSION_FILE = ".fallow-version";
const SIGNATURE_SUFFIX = ".sig";
const SHA256_SUFFIX = ".sha256";

// Cross-process install lock. Multiple VS Code windows share one global-storage
// `bin/` directory, so a simultaneous post-release restore would otherwise have
// every window download to the same final paths at once. On Windows the loser of
// that race fails with EPERM/EBUSY/EACCES because the target is already open
// (issue #1091). The lock serializes installs to a single window; the others
// wait, then reuse what the winner installed.
const INSTALL_LOCK_FILE = ".install.lock";
// Longer than any realistic single-binary download, so a slow-but-live install
// is never mistaken for a crashed one. The atomic temp+rename keeps even a
// stolen-lock race from corrupting the binary.
const STALE_LOCK_MS = 120_000;
const LOCK_POLL_MS = 200;
// Hard ceiling on waiting for a sibling before stealing the lock, to guarantee
// forward progress even if a holder's clock or mtime is pathological.
const LOCK_MAX_WAIT_MS = 180_000;

// Per-process counter so concurrent temp paths within one process never collide.
let tempCounter = 0;
const BINARY_SIGNING_PUBLIC_KEY = Buffer.from([
  131, 78, 111, 215, 115, 51, 230, 238, 223, 119, 147, 71, 199, 16, 172, 180, 3, 210, 216, 35, 77,
  85, 159, 94, 215, 200, 126, 85, 42, 222, 11, 209,
]);
const ED25519_SPKI_HEADER = Buffer.from([
  0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
]);

interface GithubRelease {
  readonly tag_name: string;
  readonly assets: ReadonlyArray<{
    readonly digest?: string;
    readonly name: string;
    readonly browser_download_url: string;
  }>;
}

const REQUEST_HEADERS = { "User-Agent": "fallow-vscode" };
const EXTENSION_ID = "fallow-rs.fallow-vscode";

/** Outcome of one locked LSP+CLI install attempt. */
type LspCliInstall =
  | { kind: "lsp-missing"; tag: string }
  | {
      kind: "ready";
      lspPath: string;
      lspDownloaded: boolean;
      cliPath: string | null;
      cliDownloaded: boolean;
      cliError: string | null;
      tag: string;
    };

export const platformTargetFor = (platform: NodeJS.Platform, arch: string): string | null => {
  if (platform === "darwin" && arch === "arm64") return "darwin-arm64";
  if (platform === "darwin" && arch === "x64") return "darwin-x64";
  if (platform === "linux" && arch === "x64") return "linux-x64-gnu";
  if (platform === "linux" && arch === "arm64") return "linux-arm64-gnu";
  if (platform === "win32" && arch === "arm64") return "win32-arm64-msvc";
  if (platform === "win32" && arch === "x64") return "win32-x64-msvc";

  return null;
};

const getPlatformTarget = (): string | null => platformTargetFor(os.platform(), os.arch());

const withRedirects = <T>(
  url: string,
  handleResponse: (response: IncomingMessage) => Promise<T>,
): Promise<T> =>
  new Promise((resolve, reject) => {
    const request = https.get(url, { headers: REQUEST_HEADERS }, (response) => {
      if (
        response.statusCode &&
        response.statusCode >= 300 &&
        response.statusCode < 400 &&
        response.headers.location
      ) {
        response.resume();
        withRedirects(response.headers.location, handleResponse).then(resolve, reject);
        return;
      }

      if (response.statusCode && response.statusCode >= 400) {
        response.resume();
        reject(new Error(`HTTP ${response.statusCode}`));
        return;
      }

      void handleResponse(response).then(resolve, reject);
    });

    request.on("error", reject);
  });

const httpsGet = (url: string): Promise<string> =>
  withRedirects(url, async (response) => {
    const chunks: Buffer[] = [];

    return await new Promise<string>((resolve, reject) => {
      response.on("data", (chunk: Buffer) => chunks.push(chunk));
      response.on("end", () => resolve(Buffer.concat(chunks).toString()));
      response.on("error", reject);
    });
  });

export const httpsDownload = (url: string, dest: string): Promise<void> =>
  withRedirects(
    url,
    async (response) =>
      await new Promise<void>((resolve, reject) => {
        const file = fs.createWriteStream(dest);
        // Guard the readable (response) stream as well: `pipe()` does not forward
        // a readable's errors to the writable, so a mid-download socket drop would
        // emit an unhandled `error` on `response` and crash the whole extension
        // host. Tear down the write stream, drop the partial file, and reject.
        response.on("error", (err) => {
          file.destroy();
          fs.unlink(dest, () => {});
          reject(err);
        });
        response.pipe(file);
        file.on("finish", () => {
          file.close();
          resolve();
        });
        file.on("error", (err) => {
          fs.unlink(dest, () => {});
          reject(err);
        });
      }),
  );

const getInstallDir = (context: vscode.ExtensionContext): string => {
  const dir = path.join(context.globalStorageUri.fsPath, "bin");
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }
  return dir;
};

const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

const getLockPath = (dir: string): string => path.join(dir, INSTALL_LOCK_FILE);

/**
 * Atomically create the install lock. `wx` opens with O_CREAT|O_EXCL, so exactly
 * one process across all windows can create the file. Returns false when another
 * window already holds it (EEXIST); rethrows other errors so the caller can fall
 * back to a lock-free install.
 */
export const tryAcquireInstallLock = (lockPath: string): boolean => {
  try {
    const fd = fs.openSync(lockPath, "wx");
    try {
      fs.writeSync(fd, `${process.pid}`);
    } finally {
      fs.closeSync(fd);
    }
    return true;
  } catch (err) {
    if ((err as NodeJS.ErrnoException).code === "EEXIST") {
      return false;
    }
    throw err;
  }
};

const releaseInstallLock = (lockPath: string): void => {
  try {
    fs.unlinkSync(lockPath);
  } catch {
    // Best-effort: the lock may already be gone (stolen as stale by a sibling).
  }
};

/** A lock whose mtime is older than the stale threshold was left by a crashed window. */
const isInstallLockStale = (lockPath: string): boolean => {
  try {
    return Date.now() - fs.statSync(lockPath).mtimeMs > STALE_LOCK_MS;
  } catch {
    // Vanished between checks: not stale, just gone.
    return false;
  }
};

/**
 * Run `fn` while holding the cross-process install lock. Waits for a live sibling
 * to finish, steals a stale or long-overdue lock to guarantee progress, and
 * degrades to a lock-free run if the lock cannot be managed at all (e.g. a
 * read-only storage dir). Correctness never depends on the lock: callers
 * double-check the installed binary inside `fn` and publish atomically.
 */
export const withInstallLock = async <T>(dir: string, fn: () => Promise<T>): Promise<T> => {
  const lockPath = getLockPath(dir);
  const deadline = Date.now() + LOCK_MAX_WAIT_MS;

  for (;;) {
    let acquired = false;
    try {
      acquired = tryAcquireInstallLock(lockPath);
    } catch {
      // The lock cannot be created here. The atomic temp+rename install still
      // prevents corruption, so proceed without serialization.
      return fn();
    }

    if (acquired) {
      try {
        return await fn();
      } finally {
        releaseInstallLock(lockPath);
      }
    }

    if (isInstallLockStale(lockPath) || Date.now() >= deadline) {
      releaseInstallLock(lockPath);
      continue;
    }

    await sleep(LOCK_POLL_MS);
  }
};

const uniqueTempPath = (dir: string, name: string): string => {
  tempCounter += 1;
  return path.join(dir, `.${name}.${process.pid}.${tempCounter}.tmp`);
};

const fileDigestHex = (filePath: string): string =>
  createHash("sha256").update(fs.readFileSync(filePath)).digest("hex");

/** Publish a small sidecar (signature / digest marker) by replacing any prior copy. */
const publishSidecar = (tempPath: string, finalPath: string): void => {
  try {
    if (fs.existsSync(finalPath)) {
      fs.unlinkSync(finalPath);
    }
  } catch {
    // Best-effort: rename below will surface a real failure.
  }
  fs.renameSync(tempPath, finalPath);
};

/**
 * Atomically move a verified temp binary onto its final path. If the rename fails
 * because the target is locked (a sibling window's running LSP on Windows) but the
 * on-disk binary is already byte-identical to what we just verified, treat it as a
 * successful install instead of throwing (issue #1091).
 */
export const renameIntoPlace = (tempPath: string, finalPath: string): void => {
  try {
    fs.renameSync(tempPath, finalPath);
  } catch (err) {
    if (fs.existsSync(finalPath) && fileDigestHex(finalPath) === fileDigestHex(tempPath)) {
      try {
        fs.unlinkSync(tempPath);
      } catch {
        // Best-effort cleanup of the now-redundant temp copy.
      }
      return;
    }
    throw err;
  }
};

const getSignaturePath = (binaryPath: string): string => `${binaryPath}${SIGNATURE_SUFFIX}`;

const getDigestPath = (binaryPath: string): string => `${binaryPath}${SHA256_SUFFIX}`;

const purgeManagedBinary = (binaryPath: string): void => {
  for (const candidate of [binaryPath, getSignaturePath(binaryPath), getDigestPath(binaryPath)]) {
    try {
      if (fs.existsSync(candidate)) {
        fs.unlinkSync(candidate);
      }
    } catch {
      // Best-effort cleanup.
    }
  }
};

export const writeVersionMarker = (dir: string, version: string): void => {
  try {
    fs.writeFileSync(path.join(dir, VERSION_FILE), version, "utf-8");
  } catch {
    // Best-effort. Next activation falls back to --version.
  }
};

export const readVersionMarker = (dir: string): string | null => {
  try {
    return fs.readFileSync(path.join(dir, VERSION_FILE), "utf-8").trim() || null;
  } catch {
    return null;
  }
};

/** Query the version of a fallow binary. Returns the version string or null. */
export const getBinaryVersion = (binaryPath: string): string | null => {
  try {
    // execFileSync is safe because there is no shell and the path is from our own storage dir.
    const output = execFileSync(binaryPath, ["--version"], {
      timeout: 5000,
      encoding: "utf-8",
    });
    // Anchor to fallow's clap `--version` shape ("fallow 2.88.1",
    // "fallow-lsp 2.88.1", "fallow-mcp 2.88.1"). Matching a bare semver anywhere
    // in the output mistook unrelated numbers for the version: a resolved npm
    // shim that cannot find its platform binary, or a crash, can surface a Node
    // banner ("Node.js v22.22.1"), and the shim appends a "verified: ..." line;
    // any of those could win the old loose match and produce a bogus
    // version-mismatch warning. Require the binary-name prefix, and return null
    // (treated as "unknown") when no fallow version line is present.
    const match = output.match(/^fallow(?:-lsp|-mcp)?\s+v?(\d+\.\d+\.\d+)/m);
    return match?.[1] ?? null;
  } catch {
    return null;
  }
};

export const verifyBinarySignature = (binaryPath: string): boolean => {
  try {
    const signaturePath = getSignaturePath(binaryPath);
    const binaryBytes = fs.readFileSync(binaryPath);
    const signatureBytes = fs.readFileSync(signaturePath);

    const publicKey = createPublicKey({
      key: Buffer.concat([ED25519_SPKI_HEADER, BINARY_SIGNING_PUBLIC_KEY]),
      format: "der",
      type: "spki",
    });

    return verify(null, binaryBytes, publicKey, signatureBytes);
  } catch {
    return false;
  }
};

const normalizeSha256Digest = (digest: string | undefined): string | null => {
  if (!digest) {
    return null;
  }

  const lower = digest.trim().toLowerCase();
  if (!lower.startsWith("sha256:")) {
    return null;
  }

  const value = lower.slice("sha256:".length);
  return /^[0-9a-f]{64}$/.test(value) ? value : null;
};

const writeDigestMarker = (binaryPath: string, digest: string): void => {
  try {
    fs.writeFileSync(getDigestPath(binaryPath), digest, "utf-8");
  } catch {
    // Best-effort. A missing digest marker forces a re-download later.
  }
};

const readDigestMarker = (binaryPath: string): string | null => {
  try {
    return normalizeSha256Digest(
      `sha256:${fs.readFileSync(getDigestPath(binaryPath), "utf-8").trim()}`,
    );
  } catch {
    return null;
  }
};

export const verifyBinaryDigest = (binaryPath: string, expectedDigest: string): boolean => {
  try {
    const normalized = normalizeSha256Digest(`sha256:${expectedDigest}`);
    if (!normalized) {
      return false;
    }

    const binaryBytes = fs.readFileSync(binaryPath);
    const actual = createHash("sha256").update(binaryBytes).digest("hex");
    return actual === normalized;
  } catch {
    return false;
  }
};

const ensureManagedBinaryTrusted = (
  binaryPath: string,
  label: string,
  outputChannel?: vscode.OutputChannel,
): boolean => {
  const signaturePath = getSignaturePath(binaryPath);
  if (fs.existsSync(signaturePath)) {
    if (verifyBinarySignature(binaryPath)) {
      return true;
    }

    outputChannel?.appendLine(
      `Fallow: installed ${label} binary failed Ed25519 signature verification. Re-downloading.`,
    );
    purgeManagedBinary(binaryPath);
    return false;
  }

  const expectedDigest = readDigestMarker(binaryPath);
  if (expectedDigest && verifyBinaryDigest(binaryPath, expectedDigest)) {
    outputChannel?.appendLine(
      `Fallow: installed ${label} binary reused via stored SHA-256 digest verification.`,
    );
    return true;
  }

  outputChannel?.appendLine(
    `Fallow: installed ${label} binary is neither signature-verified nor digest-verified. Re-downloading.`,
  );
  purgeManagedBinary(binaryPath);
  return false;
};

export const getExtensionVersion = (): string | null => {
  const version = vscode.extensions.getExtension(EXTENSION_ID)?.packageJSON?.version as
    | string
    | undefined;
  return version?.trim() || null;
};

export const releaseApiUrlForVersion = (version: string | null): string =>
  version
    ? `https://api.github.com/repos/${GITHUB_REPO}/releases/tags/v${version}`
    : `https://api.github.com/repos/${GITHUB_REPO}/releases/latest`;

const normalizeReleaseVersion = (release: GithubRelease): string =>
  release.tag_name.replace(/^v/, "").trim();

const fetchReleaseForExtension = async (): Promise<GithubRelease> => {
  const releaseJson = await httpsGet(releaseApiUrlForVersion(getExtensionVersion()));
  return JSON.parse(releaseJson) as GithubRelease;
};

export const matchesExtensionVersion = (
  dir: string,
  binaryPath: string,
  label: string,
  outputChannel?: vscode.OutputChannel,
): boolean => {
  const extensionVersion = getExtensionVersion();
  if (!extensionVersion) {
    return true;
  }

  const markerVersion = readVersionMarker(dir);
  const binaryVersion = getBinaryVersion(binaryPath);

  if (binaryVersion === extensionVersion) {
    return true;
  }

  if (!binaryVersion && markerVersion === extensionVersion) {
    return true;
  }

  outputChannel?.appendLine(
    `Fallow: installed ${label} binary is v${binaryVersion ?? markerVersion ?? "unknown"}, extension is v${extensionVersion}. Re-downloading.`,
  );
  // Purge ONLY the mismatched binary, not the whole managed set. `downloadBinary`
  // verifies the LSP first, then checks the CLI; purging both here would delete
  // the already-verified LSP and return its now-stale path. A fresh version
  // marker is rewritten on the next successful download.
  purgeManagedBinary(binaryPath);
  return false;
};

const getManagedBinaryPath = (
  context: vscode.ExtensionContext,
  binaryName: string,
  label: string,
  outputChannel?: vscode.OutputChannel,
): string | null => {
  const dir = getInstallDir(context);
  const binaryPath = path.join(dir, `${binaryName}${getExecutableExtension()}`);
  if (!fs.existsSync(binaryPath)) {
    return null;
  }

  if (!ensureManagedBinaryTrusted(binaryPath, label, outputChannel)) {
    return null;
  }

  if (!matchesExtensionVersion(dir, binaryPath, label, outputChannel)) {
    return null;
  }

  return binaryPath;
};

export const getInstalledBinaryPath = (
  context: vscode.ExtensionContext,
  outputChannel?: vscode.OutputChannel,
): string | null => getManagedBinaryPath(context, LSP_BINARY_NAME, "LSP", outputChannel);

export const getInstalledCliPath = (
  context: vscode.ExtensionContext,
  outputChannel?: vscode.OutputChannel,
): string | null => getManagedBinaryPath(context, CLI_BINARY_NAME, "CLI", outputChannel);

/** Download a single binary asset from a GitHub release. Returns the dest path or null. */
const downloadAsset = async (
  release: GithubRelease,
  binaryName: string,
  target: string,
  dir: string,
): Promise<string | null> => {
  const extension = getExecutableExtension();
  const assetName = `${binaryName}-${target}${extension}`;
  const asset = release.assets.find((a) => a.name === assetName);

  if (!asset) {
    return null;
  }

  const signatureAsset = release.assets.find(
    (candidate) => candidate.name === `${assetName}${SIGNATURE_SUFFIX}`,
  );
  const expectedDigest = normalizeSha256Digest(asset.digest);

  const destPath = path.join(dir, `${binaryName}${extension}`);
  const signaturePath = getSignaturePath(destPath);
  const digestPath = getDigestPath(destPath);

  // Download and verify on unique temp paths, then publish atomically. Two
  // windows that reach this point concurrently (after a stolen or unavailable
  // lock) write to different temp files and never truncate the same in-use
  // binary, which is the Windows EPERM/EBUSY trigger from issue #1091.
  const tempBinary = uniqueTempPath(dir, binaryName);
  const tempSignature = getSignaturePath(tempBinary);
  const tempDigest = getDigestPath(tempBinary);
  // Track whether we replaced the final sidecars so a later failure can roll
  // them back instead of leaving a sidecar that points at a missing binary.
  let sidecarsPublished = false;

  try {
    await httpsDownload(asset.browser_download_url, tempBinary);

    if (signatureAsset) {
      await httpsDownload(signatureAsset.browser_download_url, tempSignature);

      // verifyBinarySignature reads `${tempBinary}.sig`, i.e. tempSignature.
      if (!verifyBinarySignature(tempBinary)) {
        throw new Error(`${assetName} failed Ed25519 signature verification`);
      }
    } else if (expectedDigest) {
      if (!verifyBinaryDigest(tempBinary, expectedDigest)) {
        throw new Error(`${assetName} failed SHA-256 digest verification`);
      }
      // Stage the digest marker next to the temp binary so it publishes the
      // same way the signature does.
      writeDigestMarker(tempBinary, expectedDigest);
    } else {
      throw new Error(`${assetName} is missing both a signature asset and a GitHub release digest`);
    }

    if (os.platform() !== "win32") {
      fs.chmodSync(tempBinary, 0o755);
    }

    // Publish sidecars to their final paths BEFORE the binary, then rename the
    // binary last. A reader in another window gates on the binary existing, so
    // it never observes a binary without its signature/digest.
    if (signatureAsset) {
      publishSidecar(tempSignature, signaturePath);
      if (fs.existsSync(digestPath)) {
        fs.unlinkSync(digestPath);
      }
    } else {
      publishSidecar(tempDigest, digestPath);
      if (fs.existsSync(signaturePath)) {
        fs.unlinkSync(signaturePath);
      }
    }
    sidecarsPublished = true;

    renameIntoPlace(tempBinary, destPath);
  } catch (error) {
    const candidates = [tempBinary, tempSignature, tempDigest];
    // If we published sidecars but the binary never landed, the final sidecars
    // now describe a missing binary; roll them back so the next activation does
    // not reuse or trip over an orphan marker.
    if (sidecarsPublished) {
      candidates.push(signaturePath, digestPath);
    }
    for (const candidate of candidates) {
      try {
        if (fs.existsSync(candidate)) {
          fs.unlinkSync(candidate);
        }
      } catch {
        // Best-effort cleanup on failed downloads.
      }
    }
    throw error;
  }

  return destPath;
};

const promptAfterDownloadFailure = async (message: string): Promise<boolean> => {
  const choice = await vscode.window.showErrorMessage(
    message,
    "Retry",
    "Open Settings",
    "Show Output",
  );

  if (choice === "Open Settings") {
    void vscode.commands.executeCommand("workbench.action.openSettings", "fallow");
  }

  if (choice === "Show Output") {
    void vscode.commands.executeCommand("fallow.showOutput");
  }

  return choice === "Retry";
};

const ensurePlatformTarget = (): string | null => {
  const target = getPlatformTarget();
  if (!target) {
    void vscode.window.showErrorMessage(
      `Fallow: unsupported platform ${os.platform()}-${os.arch()}`,
    );
    return null;
  }

  return target;
};

const downloadManagedBinary = async (
  context: vscode.ExtensionContext,
  binaryName: string,
  label: string,
): Promise<string | null> => {
  const target = ensurePlatformTarget();
  if (!target) {
    return null;
  }

  return vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: `Fallow: Downloading ${label} binary...`,
      cancellable: false,
    },
    async () => {
      for (;;) {
        const dir = getInstallDir(context);
        try {
          // The lock holds only for one download attempt; user prompts run
          // outside it so a modal never blocks a sibling window.
          const result = await withInstallLock(
            dir,
            async (): Promise<{ path: string; toast: string | null } | { tag: string }> => {
              // A sibling window may have installed it while we waited for the
              // lock; reuse it instead of downloading again.
              const existing = getManagedBinaryPath(context, binaryName, label);
              if (existing) {
                return { path: existing, toast: null };
              }

              const release = await fetchReleaseForExtension();
              const binaryPath = await downloadAsset(release, binaryName, target, dir);
              if (!binaryPath) {
                return { tag: release.tag_name };
              }

              writeVersionMarker(dir, normalizeReleaseVersion(release));
              return {
                path: binaryPath,
                toast: `Fallow: ${label} ${release.tag_name} installed.`,
              };
            },
          );

          if ("tag" in result) {
            const shouldRetry = await promptAfterDownloadFailure(
              `Fallow: no ${label} binary found for ${target} in release ${result.tag}.`,
            );
            if (shouldRetry) {
              continue;
            }
            return null;
          }

          if (result.toast) {
            void vscode.window.showInformationMessage(result.toast);
          }
          return result.path;
        } catch (err) {
          const message = err instanceof Error ? err.message : String(err);
          const shouldRetry = await promptAfterDownloadFailure(
            `Fallow: failed to download ${label} binary: ${message}`,
          );
          if (!shouldRetry) {
            return null;
          }
        }
      }
    },
  );
};

export const downloadCliBinary = async (context: vscode.ExtensionContext): Promise<string | null> =>
  downloadManagedBinary(context, CLI_BINARY_NAME, "CLI");

export const downloadBinary = async (context: vscode.ExtensionContext): Promise<string | null> => {
  const target = ensurePlatformTarget();
  if (!target) {
    return null;
  }

  return vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: "Fallow: Downloading binaries...",
      cancellable: false,
    },
    async () => {
      let cliRetried = false;

      for (;;) {
        const dir = getInstallDir(context);
        try {
          // One lock per attempt; prompts run outside the lock. The locked body
          // double-checks each binary so a sibling-installed copy is reused.
          const result = await withInstallLock(dir, async (): Promise<LspCliInstall> => {
            let lspPath = getManagedBinaryPath(context, LSP_BINARY_NAME, "LSP");
            let lspDownloaded = false;
            let release: GithubRelease | null = null;

            if (!lspPath) {
              release = await fetchReleaseForExtension();
              lspPath = await downloadAsset(release, LSP_BINARY_NAME, target, dir);
              if (!lspPath) {
                return { kind: "lsp-missing", tag: release.tag_name };
              }
              writeVersionMarker(dir, normalizeReleaseVersion(release));
              lspDownloaded = true;
            }

            let cliPath = getManagedBinaryPath(context, CLI_BINARY_NAME, "CLI");
            let cliDownloaded = false;
            let cliError: string | null = null;

            if (!cliPath) {
              if (!release) {
                release = await fetchReleaseForExtension();
              }
              try {
                cliPath = await downloadAsset(release, CLI_BINARY_NAME, target, dir);
                cliDownloaded = cliPath !== null;
              } catch (cliErr) {
                cliError = cliErr instanceof Error ? cliErr.message : String(cliErr);
              }
            }

            const tag = release ? release.tag_name : (readVersionMarker(dir) ?? "");
            return { kind: "ready", lspPath, lspDownloaded, cliPath, cliDownloaded, cliError, tag };
          });

          if (result.kind === "lsp-missing") {
            const shouldRetry = await promptAfterDownloadFailure(
              `Fallow: no LSP binary found for ${target} in release ${result.tag}.`,
            );
            if (shouldRetry) {
              continue;
            }
            return null;
          }

          if (!result.cliPath) {
            // CLI is best-effort: prompt once, then warn and keep the LSP.
            if (!cliRetried) {
              cliRetried = true;
              const reason = result.cliError
                ? `Fallow: failed to download CLI binary: ${result.cliError}`
                : `Fallow: no CLI binary found for ${target} in release ${result.tag}. Tree views and fix commands require the fallow CLI.`;
              const shouldRetry = await promptAfterDownloadFailure(reason);
              if (shouldRetry) {
                continue;
              }
            }
            void vscode.window.showWarningMessage(
              `Fallow: LSP ${result.tag} installed. CLI binary is still missing, so tree views and fix commands need another CLI source.`,
            );
            return result.lspPath;
          }

          if (result.lspDownloaded || result.cliDownloaded) {
            void vscode.window.showInformationMessage(
              `Fallow: ${result.tag} installed (LSP + CLI).`,
            );
          }
          return result.lspPath;
        } catch (err) {
          const message = err instanceof Error ? err.message : String(err);
          const shouldRetry = await promptAfterDownloadFailure(
            `Fallow: failed to download binaries: ${message}`,
          );
          if (!shouldRetry) {
            return null;
          }
        }
      }
    },
  );
};
