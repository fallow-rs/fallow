const typeAwareCommand = (
  companion,
  { platform = process.platform, execPath = process.execPath } = {},
) => {
  if (platform === "win32") {
    return { binary: execPath, script: companion };
  }
  return { binary: companion, script: null };
};

const configureTypeAwareCommand = (companion) => {
  const command = typeAwareCommand(companion);
  process.env.FALLOW_TYPE_AWARE_BIN = command.binary;
  if (command.script) {
    process.env.FALLOW_TYPE_AWARE_SCRIPT = command.script;
  }
};

module.exports = { configureTypeAwareCommand, typeAwareCommand };
