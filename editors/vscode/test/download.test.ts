import * as path from "node:path";
import { describe, expect, it, vi, beforeEach } from "vitest";

let mockFiles: Record<string, string | Buffer> = {};
let mockExecOutput = "";
let mockExecError = false;
let mockSignatureValid = true;
let mockHashInput = "";

vi.mock("node:fs", () => ({
  existsSync: (p: string) => p in mockFiles,
  readFileSync: (p: string) => {
    if (p in mockFiles) return mockFiles[p];
    throw new Error("ENOENT");
  },
  writeFileSync: (p: string, content: string | Buffer) => {
    mockFiles[p] = content;
  },
  unlinkSync: (p: string) => {
    delete mockFiles[p];
  },
  mkdirSync: () => {},
}));

vi.mock("node:child_process", () => ({
  execFileSync: () => {
    if (mockExecError) throw new Error("exec failed");
    return mockExecOutput;
  },
}));

vi.mock("node:crypto", () => ({
  createPublicKey: () => ({ type: "mock-public-key" }),
  createHash: () => ({
    update(data: string | Buffer) {
      mockHashInput = Buffer.isBuffer(data) ? data.toString("utf8") : data;
      return this;
    },
    digest(encoding: string) {
      if (encoding !== "hex") {
        throw new Error("unsupported encoding");
      }

      return Buffer.from(mockHashInput, "utf8")
        .toString("hex")
        .padEnd(64, "0")
        .slice(0, 64);
    },
  }),
  verify: () => mockSignatureValid,
}));

vi.mock("vscode", () => ({
  extensions: {
    getExtension: () => ({
      packageJSON: { version: "2.26.0" },
    }),
  },
}));

import {
  getInstalledBinaryPath,
  getInstalledCliPath,
  getBinaryVersion,
  matchesExtensionVersion,
  platformTargetFor,
  readVersionMarker,
  releaseApiUrlForVersion,
  verifyBinaryDigest,
  verifyBinarySignature,
  writeVersionMarker,
} from "../src/download.js";

const fakeContext = {
  globalStorageUri: { fsPath: "/storage" },
} as any;

const binDir = path.join("/storage", "bin");
const lspPath = path.join(binDir, "fallow-lsp");
const cliPath = path.join(binDir, "fallow");
const lspSigPath = `${lspPath}.sig`;
const cliSigPath = `${cliPath}.sig`;
const lspDigestPath = `${lspPath}.sha256`;
const cliDigestPath = `${cliPath}.sha256`;
const versionPath = path.join(binDir, ".fallow-version");
const binaryBytes = Buffer.from("signed-binary");
const signatureBytes = Buffer.alloc(64, 1);
const digestHex = Buffer.from("signed-binary", "utf8")
  .toString("hex")
  .padEnd(64, "0")
  .slice(0, 64);

describe("writeVersionMarker / readVersionMarker", () => {
  beforeEach(() => {
    mockFiles = {};
  });

  it("round-trips a version string", () => {
    writeVersionMarker(binDir, "2.26.1");
    expect(readVersionMarker(binDir)).toBe("2.26.1");
  });

  it("returns null when no marker exists", () => {
    expect(readVersionMarker(binDir)).toBeNull();
  });

  it("trims whitespace from marker content", () => {
    mockFiles[versionPath] = "  2.26.1\n";
    expect(readVersionMarker(binDir)).toBe("2.26.1");
  });

  it("returns null for empty marker file", () => {
    mockFiles[versionPath] = "  ";
    expect(readVersionMarker(binDir)).toBeNull();
  });
});

describe("matchesExtensionVersion", () => {
  beforeEach(() => {
    mockFiles = {};
    mockExecOutput = "";
    mockExecError = false;
  });

  it("purges ONLY the mismatched binary, sparing the verified sibling and the marker", () => {
    for (const f of [
      lspPath,
      lspSigPath,
      lspDigestPath,
      cliPath,
      cliSigPath,
      cliDigestPath,
      versionPath,
    ]) {
      mockFiles[f] = "x";
    }
    // Extension is 2.26.0 (vscode mock); the CLI reports a stale version.
    mockExecOutput = "fallow 2.25.0\n";

    const ok = matchesExtensionVersion(binDir, cliPath, "CLI");

    expect(ok).toBe(false);
    // The mismatched CLI binary + its sidecars are removed.
    expect(cliPath in mockFiles).toBe(false);
    expect(cliSigPath in mockFiles).toBe(false);
    expect(cliDigestPath in mockFiles).toBe(false);
    // The already-verified LSP binary, its sidecars, and the version marker
    // MUST survive: downloadBinary verifies the LSP first, so purging the whole
    // set here would delete it and then return its now-deleted path.
    expect(lspPath in mockFiles).toBe(true);
    expect(lspSigPath in mockFiles).toBe(true);
    expect(lspDigestPath in mockFiles).toBe(true);
    expect(versionPath in mockFiles).toBe(true);
  });

  it("returns true (no purge) when the binary version matches the extension", () => {
    mockFiles[cliPath] = "x";
    mockExecOutput = "fallow 2.26.0\n";
    expect(matchesExtensionVersion(binDir, cliPath, "CLI")).toBe(true);
    expect(cliPath in mockFiles).toBe(true);
  });
});

describe("getBinaryVersion", () => {
  beforeEach(() => {
    mockExecOutput = "";
    mockExecError = false;
    mockSignatureValid = true;
    mockHashInput = "";
  });

  it("parses version from fallow-lsp output", () => {
    mockExecOutput = "fallow-lsp 2.25.0\n";
    expect(getBinaryVersion("/bin/fallow-lsp")).toBe("2.25.0");
  });

  it("parses version from fallow CLI output", () => {
    mockExecOutput = "fallow 2.88.1\n";
    expect(getBinaryVersion("/bin/fallow")).toBe("2.88.1");
  });

  it("ignores the npm shim's appended verified line", () => {
    mockExecOutput = "fallow-lsp 2.88.1\nverified: yes (sentinel /Users/me/.cache/2.0.0/x)\n";
    expect(getBinaryVersion("/bin/fallow-lsp")).toBe("2.88.1");
  });

  it("does not mistake a Node crash banner for the fallow version", () => {
    // A resolved npm shim that cannot find its platform binary can surface a
    // Node banner; that bare semver must not be read as the fallow version.
    mockExecOutput = "Cannot find module 'detect-libc'\nNode.js v22.22.1\n";
    expect(getBinaryVersion("/bin/fallow-lsp")).toBeNull();
  });

  it("does not match a stray semver in a sentinel path with no version line", () => {
    mockExecOutput = "verified: yes (sentinel /Users/me/.cache/fallow/2.0.0/sentinel)\n";
    expect(getBinaryVersion("/bin/fallow-lsp")).toBeNull();
  });

  it("returns null on exec failure", () => {
    mockExecError = true;
    expect(getBinaryVersion("/bin/fallow-lsp")).toBeNull();
  });

  it("returns null on unparsable output", () => {
    mockExecOutput = "unknown";
    expect(getBinaryVersion("/bin/fallow-lsp")).toBeNull();
  });
});

describe("verifyBinarySignature", () => {
  beforeEach(() => {
    mockFiles = {};
    mockSignatureValid = true;
  });

  it("returns true when the binary and signature verify", () => {
    mockFiles[lspPath] = binaryBytes;
    mockFiles[lspSigPath] = signatureBytes;

    expect(verifyBinarySignature(lspPath)).toBe(true);
  });

  it("returns false when the signature file is missing", () => {
    mockFiles[lspPath] = binaryBytes;

    expect(verifyBinarySignature(lspPath)).toBe(false);
  });

  it("returns false when crypto verification fails", () => {
    mockFiles[lspPath] = binaryBytes;
    mockFiles[lspSigPath] = signatureBytes;
    mockSignatureValid = false;

    expect(verifyBinarySignature(lspPath)).toBe(false);
  });
});

describe("verifyBinaryDigest", () => {
  beforeEach(() => {
    mockFiles = {};
    mockHashInput = "";
  });

  it("returns true when the stored digest matches the binary", () => {
    mockFiles[lspPath] = binaryBytes;

    expect(verifyBinaryDigest(lspPath, digestHex)).toBe(true);
  });

  it("returns false when the digest does not match", () => {
    mockFiles[lspPath] = binaryBytes;

    expect(verifyBinaryDigest(lspPath, "0".repeat(64))).toBe(false);
  });
});

describe("platformTargetFor", () => {
  it("maps Windows arm64 to the MSVC target", () => {
    expect(platformTargetFor("win32", "arm64")).toBe("win32-arm64-msvc");
  });

  it("keeps existing Windows x64 mapping", () => {
    expect(platformTargetFor("win32", "x64")).toBe("win32-x64-msvc");
  });

  it("returns null for unsupported targets", () => {
    expect(platformTargetFor("win32", "ia32")).toBeNull();
    expect(platformTargetFor("freebsd", "x64")).toBeNull();
  });
});

describe("releaseApiUrlForVersion", () => {
  it("targets the release tag when the extension version is known", () => {
    expect(releaseApiUrlForVersion("2.26.0")).toBe(
      "https://api.github.com/repos/fallow-rs/fallow/releases/tags/v2.26.0",
    );
  });

  it("falls back to the latest release only when the extension version is unavailable", () => {
    expect(releaseApiUrlForVersion(null)).toBe(
      "https://api.github.com/repos/fallow-rs/fallow/releases/latest",
    );
  });
});

describe("getInstalledBinaryPath", () => {
  beforeEach(() => {
    mockFiles = {};
    mockExecOutput = "";
    mockExecError = false;
    mockSignatureValid = true;
  });

  it("returns null when no binary exists", () => {
    expect(getInstalledBinaryPath(fakeContext)).toBeNull();
  });

  it("returns path when version marker matches", () => {
    mockFiles[lspPath] = binaryBytes;
    mockFiles[lspSigPath] = signatureBytes;
    mockFiles[versionPath] = "2.26.0";

    expect(getInstalledBinaryPath(fakeContext)).toBe(lspPath);
  });

  it("returns null and deletes ONLY the stale LSP binary (sibling CLI + marker survive)", () => {
    mockFiles[lspPath] = binaryBytes;
    mockFiles[lspSigPath] = signatureBytes;
    mockFiles[cliPath] = binaryBytes;
    mockFiles[cliSigPath] = signatureBytes;
    mockFiles[versionPath] = "2.25.0";

    expect(getInstalledBinaryPath(fakeContext)).toBeNull();
    // The mismatched LSP binary + its sidecar are purged.
    expect(mockFiles[lspPath]).toBeUndefined();
    expect(mockFiles[lspSigPath]).toBeUndefined();
    // Per-binary purge (not whole-set): the CLI binary and the version marker
    // survive so a CLI check cannot return an already-deleted path.
    expect(mockFiles[cliPath]).not.toBeUndefined();
    expect(mockFiles[cliSigPath]).not.toBeUndefined();
    expect(mockFiles[versionPath]).not.toBeUndefined();
  });

  it("falls back to --version when no marker exists", () => {
    mockFiles[lspPath] = binaryBytes;
    mockFiles[lspSigPath] = signatureBytes;
    mockExecOutput = "fallow-lsp 2.26.0\n";

    expect(getInstalledBinaryPath(fakeContext)).toBe(lspPath);
  });

  it("treats unknown version as stale (null --version, no marker)", () => {
    mockFiles[lspPath] = binaryBytes;
    mockFiles[lspSigPath] = signatureBytes;
    mockExecError = true;

    expect(getInstalledBinaryPath(fakeContext)).toBeNull();
    expect(mockFiles[lspPath]).toBeUndefined();
  });

  it("treats mismatched --version as stale when no marker", () => {
    mockFiles[lspPath] = binaryBytes;
    mockFiles[lspSigPath] = signatureBytes;
    mockExecOutput = "fallow-lsp 2.24.0\n";

    expect(getInstalledBinaryPath(fakeContext)).toBeNull();
    expect(mockFiles[lspPath]).toBeUndefined();
  });

  it("treats missing signature as stale without executing the binary", () => {
    mockFiles[lspPath] = binaryBytes;
    mockExecOutput = "fallow-lsp 2.26.0\n";

    expect(getInstalledBinaryPath(fakeContext)).toBeNull();
    expect(mockFiles[lspPath]).toBeUndefined();
  });

  it("reuses a digest-verified binary when no signature file exists", () => {
    mockFiles[lspPath] = binaryBytes;
    mockFiles[lspDigestPath] = digestHex;
    mockFiles[versionPath] = "2.26.0";

    expect(getInstalledBinaryPath(fakeContext)).toBe(lspPath);
  });

  it("treats invalid signature as stale and purges only that binary", () => {
    mockFiles[lspPath] = binaryBytes;
    mockFiles[lspSigPath] = signatureBytes;
    mockFiles[cliPath] = binaryBytes;
    mockFiles[cliSigPath] = signatureBytes;
    mockFiles[versionPath] = "2.26.0";
    mockSignatureValid = false;

    expect(getInstalledBinaryPath(fakeContext)).toBeNull();
    expect(mockFiles[lspPath]).toBeUndefined();
    expect(mockFiles[cliPath]).toBe(binaryBytes);
    expect(mockFiles[versionPath]).toBe("2.26.0");
  });

  it("does not fall back to digest markers when a signature file is present but invalid", () => {
    mockFiles[lspPath] = binaryBytes;
    mockFiles[lspSigPath] = signatureBytes;
    mockFiles[lspDigestPath] = digestHex;
    mockFiles[versionPath] = "2.26.0";
    mockSignatureValid = false;

    expect(getInstalledBinaryPath(fakeContext)).toBeNull();
    expect(mockFiles[lspPath]).toBeUndefined();
    expect(mockFiles[lspSigPath]).toBeUndefined();
    expect(mockFiles[lspDigestPath]).toBeUndefined();
  });

  it("purges only the failing binary when both signature and digest verification fail", () => {
    mockFiles[lspPath] = binaryBytes;
    mockFiles[lspDigestPath] = "0".repeat(64);
    mockFiles[cliPath] = binaryBytes;
    mockFiles[cliDigestPath] = "0".repeat(64);
    mockFiles[versionPath] = "2.26.0";

    expect(getInstalledBinaryPath(fakeContext)).toBeNull();
    expect(mockFiles[lspPath]).toBeUndefined();
    expect(mockFiles[lspDigestPath]).toBeUndefined();
    expect(mockFiles[cliPath]).toBe(binaryBytes);
    expect(mockFiles[cliDigestPath]).toBe("0".repeat(64));
  });
});

describe("getInstalledCliPath", () => {
  beforeEach(() => {
    mockFiles = {};
    mockExecOutput = "";
    mockExecError = false;
    mockSignatureValid = true;
  });

  it("returns the CLI path when the managed install is signed and current", () => {
    mockFiles[cliPath] = binaryBytes;
    mockFiles[cliSigPath] = signatureBytes;
    mockFiles[versionPath] = "2.26.0";
    mockExecOutput = "fallow 2.26.0\n";

    expect(getInstalledCliPath(fakeContext)).toBe(cliPath);
  });

  it("returns the CLI path when the managed install is digest-verified and current", () => {
    mockFiles[cliPath] = binaryBytes;
    mockFiles[cliDigestPath] = digestHex;
    mockFiles[versionPath] = "2.26.0";
    mockExecOutput = "fallow 2.26.0\n";

    expect(getInstalledCliPath(fakeContext)).toBe(cliPath);
  });

  it("treats a stale CLI binary as stale even when the shared marker is current", () => {
    mockFiles[cliPath] = binaryBytes;
    mockFiles[cliSigPath] = signatureBytes;
    mockFiles[versionPath] = "2.26.0";
    mockExecOutput = "fallow 2.25.0\n";

    expect(getInstalledCliPath(fakeContext)).toBeNull();
    expect(mockFiles[cliPath]).toBeUndefined();
    expect(mockFiles[cliSigPath]).toBeUndefined();
  });

  it("retries a missing managed CLI without purging a trusted LSP binary", () => {
    mockFiles[lspPath] = binaryBytes;
    mockFiles[lspSigPath] = signatureBytes;
    mockFiles[cliPath] = binaryBytes;
    mockFiles[cliSigPath] = signatureBytes;
    mockFiles[versionPath] = "2.26.0";
    mockSignatureValid = false;

    expect(getInstalledCliPath(fakeContext)).toBeNull();
    expect(mockFiles[cliPath]).toBeUndefined();
    expect(mockFiles[lspPath]).toBe(binaryBytes);
    expect(mockFiles[lspSigPath]).toBe(signatureBytes);
  });
});
