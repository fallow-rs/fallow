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

const httpsDownload = (url: string, dest: string): Promise<void> =>
  withRedirects(
    url,
    async (response) =>
      await new Promise<void>((resolve, reject) => {
        const file = fs.createWriteStream(dest);
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

const getSignaturePath = (binaryPath: string): string => `${binaryPath}${SIGNATURE_SUFFIX}`;

const getDigestPath = (binaryPath: string): string => `${binaryPath}${SHA256_SUFFIX}`;

const getManagedBinaryPaths = (dir: string): ReadonlyArray<string> => [
  path.join(dir, `${LSP_BINARY_NAME}${getExecutableExtension()}`),
  path.join(dir, `${CLI_BINARY_NAME}${getExecutableExtension()}`),
];

const purgeManagedBinaries = (dir: string): void => {
  for (const binaryPath of getManagedBinaryPaths(dir)) {
    for (const candidate of [binaryPath, getSignaturePath(binaryPath), getDigestPath(binaryPath)]) {
      try {
        if (fs.existsSync(candidate)) {
          fs.unlinkSync(candidate);
        }
      } catch {
        // Best-effort cleanup.
      }
    }
  }

  try {
    const versionPath = path.join(dir, VERSION_FILE);
    if (fs.existsSync(versionPath)) {
      fs.unlinkSync(versionPath);
    }
  } catch {
    // Best-effort cleanup.
  }
};

export const writeVersionMarker = (dir: string, version: string): void => {
  try {
    fs.writeFileSync(path.join(dir, VERSION_FILE), version, "utf-8");
  } catch {
    // Best-effort — next activation falls back to --version
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
    // execFileSync is safe (no shell injection) — binary path is from our own storage dir.
    const output = execFileSync(binaryPath, ["--version"], {
      timeout: 5000,
      encoding: "utf-8",
    });
    // Output format: "fallow-lsp 2.18.3" or "fallow 2.18.3"
    const match = output.trim().match(/(\d+\.\d+\.\d+)/);
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
    // Best-effort — a missing digest marker forces a re-download later.
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
  dir: string,
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
    purgeManagedBinaries(dir);
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
  purgeManagedBinaries(dir);
  return false;
};

const matchesExtensionVersion = (
  dir: string,
  binaryPath: string,
  label: string,
  outputChannel?: vscode.OutputChannel,
): boolean => {
  const extensionVersion = vscode.extensions.getExtension("fallow-rs.fallow-vscode")?.packageJSON
    ?.version as string | undefined;
  if (!extensionVersion) {
    return true;
  }

  const binaryVersion = readVersionMarker(dir) ?? getBinaryVersion(binaryPath);
  if (binaryVersion === extensionVersion) {
    return true;
  }

  outputChannel?.appendLine(
    `Fallow: installed ${label} binary is v${binaryVersion ?? "unknown"}, extension is v${extensionVersion}. Re-downloading.`,
  );
  purgeManagedBinaries(dir);
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

  if (!ensureManagedBinaryTrusted(dir, binaryPath, label, outputChannel)) {
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

  try {
    await httpsDownload(asset.browser_download_url, destPath);

    if (signatureAsset) {
      await httpsDownload(signatureAsset.browser_download_url, signaturePath);

      if (!verifyBinarySignature(destPath)) {
        throw new Error(`${assetName} failed Ed25519 signature verification`);
      }

      if (fs.existsSync(digestPath)) {
        fs.unlinkSync(digestPath);
      }
    } else if (expectedDigest) {
      if (!verifyBinaryDigest(destPath, expectedDigest)) {
        throw new Error(`${assetName} failed SHA-256 digest verification`);
      }

      writeDigestMarker(destPath, expectedDigest);
    } else {
      throw new Error(`${assetName} is missing both a signature asset and a GitHub release digest`);
    }

    if (os.platform() !== "win32") {
      fs.chmodSync(destPath, 0o755);
    }
  } catch (error) {
    for (const candidate of [destPath, signaturePath, digestPath]) {
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

export const downloadBinary = async (context: vscode.ExtensionContext): Promise<string | null> => {
  const target = getPlatformTarget();
  if (!target) {
    void vscode.window.showErrorMessage(
      `Fallow: unsupported platform ${os.platform()}-${os.arch()}`,
    );
    return null;
  }

  return vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: "Fallow: Downloading binaries...",
      cancellable: false,
    },
    async () => {
      try {
        const releaseJson = await httpsGet(
          `https://api.github.com/repos/${GITHUB_REPO}/releases/latest`,
        );
        const release: GithubRelease = JSON.parse(releaseJson);
        const dir = getInstallDir(context);

        // Download LSP binary (required)
        const lspPath = await downloadAsset(release, LSP_BINARY_NAME, target, dir);
        if (!lspPath) {
          void vscode.window.showErrorMessage(
            `Fallow: no LSP binary found for ${target} in release ${release.tag_name}`,
          );
          return null;
        }

        // Write version marker so future activations can detect stale binaries
        // without needing to execute them.
        const extensionVersion = vscode.extensions.getExtension("fallow-rs.fallow-vscode")
          ?.packageJSON?.version as string | undefined;
        if (extensionVersion) {
          writeVersionMarker(dir, extensionVersion);
        }

        // Download CLI binary (best-effort — tree views and commands need it)
        let cliPath: string | null = null;
        try {
          cliPath = await downloadAsset(release, CLI_BINARY_NAME, target, dir);
        } catch (cliErr) {
          const cliMessage = cliErr instanceof Error ? cliErr.message : String(cliErr);
          void vscode.window.showWarningMessage(`Fallow: CLI download skipped: ${cliMessage}`);
        }
        if (cliPath) {
          void vscode.window.showInformationMessage(
            `Fallow: ${release.tag_name} installed (LSP + CLI).`,
          );
        } else {
          void vscode.window.showInformationMessage(
            `Fallow: LSP ${release.tag_name} installed. CLI binary not found in release — tree views require the fallow CLI in PATH.`,
          );
        }

        return lspPath;
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        void vscode.window.showErrorMessage(`Fallow: failed to download binaries: ${message}`);
        return null;
      }
    },
  );
};
