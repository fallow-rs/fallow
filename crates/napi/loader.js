const fs = require("node:fs");
const path = require("node:path");
const { configureTypeAwareCommand } = require("./type-aware-command.js");

const packageVersion = require("./package.json").version;

const existingCompanion = (manifestPath) => {
  const companion = path.join(path.dirname(manifestPath), "fallow-type-aware.mjs");
  return fs.existsSync(companion) ? companion : null;
};

const resolveTypeAwareCompanion = () => {
  try {
    const manifestPath = require.resolve("fallow-type-aware/package.json");
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    return manifest.version === packageVersion ? existingCompanion(manifestPath) : null;
  } catch {
    return null;
  }
};

const configureTypeAwareCompanion = () => {
  if (process.env.FALLOW_TYPE_AWARE_BIN) return;
  const companion = resolveTypeAwareCompanion();
  if (companion) {
    configureTypeAwareCommand(companion);
  }
};

configureTypeAwareCompanion();
module.exports = require("./index.js");
