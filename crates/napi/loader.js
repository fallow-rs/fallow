const fs = require("node:fs");
const path = require("node:path");

const packageVersion = require("./package.json").version;

function configureTypeAwareCompanion() {
  if (process.env.FALLOW_TYPE_AWARE_BIN) return;
  try {
    const manifestPath = require.resolve("fallow-type-aware/package.json");
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    if (manifest.version !== packageVersion) return;
    const companion = path.join(path.dirname(manifestPath), "fallow-type-aware.mjs");
    if (fs.existsSync(companion)) {
      process.env.FALLOW_TYPE_AWARE_BIN = companion;
    }
  } catch {
    // The companion is optional. Typed calls report an actionable error when requested.
  }
}

configureTypeAwareCompanion();
module.exports = require("./index.js");
