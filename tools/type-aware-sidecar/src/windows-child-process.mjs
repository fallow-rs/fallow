import childProcess from "node:child_process";
import { syncBuiltinESMExports } from "node:module";

const INSTALL_MARKER = Symbol.for("fallow.type-aware.windows-child-process-policy");

const hiddenOptions = (options) => ({ ...options, windowsHide: true });

/** Keep TypeScript-Go child processes hidden when the sidecar runs on Windows. */
export const installWindowsChildProcessPolicy = ({
  childProcess: processApi = childProcess,
  platform = process.platform,
  syncBuiltinESMExports: syncExports = syncBuiltinESMExports,
} = {}) => {
  if (platform !== "win32" || processApi[INSTALL_MARKER] === true) return;

  const originalSpawn = processApi.spawn;
  processApi.spawn = function spawnHidden(command, args, options) {
    if (Array.isArray(args)) {
      return originalSpawn.call(this, command, args, hiddenOptions(options));
    }
    return originalSpawn.call(this, command, hiddenOptions(args));
  };
  Object.defineProperty(processApi, INSTALL_MARKER, { value: true });
  syncExports();
};
