import { createHash } from "node:crypto";
import { readFileSync, statSync } from "node:fs";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { open } = require("yauzl");

const ZIP_HOST_UNIX = 3;

const unixMode = (entry) =>
  entry.versionMadeBy >>> 8 === ZIP_HOST_UNIX
    ? (entry.externalFileAttributes >>> 16) & 0o777
    : null;

const readArchiveEntries = (vsixPath) =>
  new Promise((resolve, reject) => {
    open(
      vsixPath,
      { lazyEntries: true, strictFileNames: true, validateEntrySizes: true },
      (openError, zipFile) => {
        if (openError) {
          reject(openError);
          return;
        }

        const entries = new Map();
        const modes = new Map();
        let settled = false;
        const fail = (error) => {
          if (settled) {
            return;
          }
          settled = true;
          zipFile.close();
          reject(error);
        };

        zipFile.on("error", fail);
        zipFile.on("end", () => {
          if (settled) {
            return;
          }
          settled = true;
          resolve({ entries, modes });
        });
        zipFile.on("entry", (entry) => {
          const path = entry.fileName.replaceAll("\\", "/").toLowerCase();
          if (entries.has(path) || modes.has(path)) {
            fail(new Error(`VSIX archive contains duplicate entry ${path}`));
            return;
          }
          if (path.endsWith("/")) {
            zipFile.readEntry();
            return;
          }
          zipFile.openReadStream(entry, (streamError, stream) => {
            if (streamError) {
              fail(streamError);
              return;
            }
            const chunks = [];
            stream.on("error", fail);
            stream.on("data", (chunk) => chunks.push(chunk));
            stream.on("end", () => {
              if (settled) {
                return;
              }
              entries.set(path, Buffer.concat(chunks));
              modes.set(path, unixMode(entry));
              zipFile.readEntry();
            });
          });
        });
        zipFile.readEntry();
      },
    );
  });

export const sha256Buffer = (contents) => createHash("sha256").update(contents).digest("hex");

export const sha256File = (path) => sha256Buffer(readFileSync(path));

export const parseVsixTargetPlatform = (xml) => {
  const identity = xml.match(/<Identity\b[^>]*>/iu)?.[0];
  if (!identity) {
    throw new Error("VSIX manifest does not contain an Identity element");
  }
  const target = identity.match(/\bTargetPlatform\s*=\s*(["'])([^"']+)\1/iu)?.[2];
  return target ?? null;
};

export const normalizedPayload = (entries) => {
  const files = [...entries.entries()]
    .filter(([path]) => path.startsWith("extension/") && !path.endsWith("/"))
    .map(([path, contents]) => ({
      path: path.replaceAll("\\", "/").toLowerCase(),
      bytes: contents.byteLength,
      sha256: sha256Buffer(contents),
    }))
    .toSorted((left, right) => left.path.localeCompare(right.path));
  const canonical = files
    .map(({ path, bytes, sha256 }) => `${JSON.stringify([path, bytes, sha256])}\n`)
    .join("");
  return {
    fileCount: files.length,
    sha256: sha256Buffer(Buffer.from(canonical, "utf8")),
    files,
  };
};

export const inspectVsixArchive = async (vsixPath) => {
  const { entries, modes } = await readArchiveEntries(vsixPath);
  const rawPackage = entries.get("extension/package.json");
  if (!rawPackage) {
    throw new Error("VSIX archive is missing extension/package.json");
  }
  const rawManifest = entries.get("extension.vsixmanifest");
  if (!rawManifest) {
    throw new Error("VSIX archive is missing extension.vsixmanifest");
  }
  const manifest = JSON.parse(rawPackage.toString("utf8"));
  return {
    bytes: statSync(vsixPath).size,
    entries,
    modes,
    payload: normalizedPayload(entries),
    sha256: sha256File(vsixPath),
    targetPlatform: parseVsixTargetPlatform(rawManifest.toString("utf8")),
    version: manifest.version,
  };
};

export const archiveFileRecord = (archive, path) => {
  const normalizedPath = path.replaceAll("\\", "/").toLowerCase();
  const contents = archive.entries.get(normalizedPath);
  if (!contents) {
    throw new Error(`VSIX archive is missing ${path}`);
  }
  return {
    path: normalizedPath.replace(/^extension\//u, ""),
    bytes: contents.byteLength,
    sha256: sha256Buffer(contents),
    mode: archive.modes.get(normalizedPath) ?? null,
  };
};
