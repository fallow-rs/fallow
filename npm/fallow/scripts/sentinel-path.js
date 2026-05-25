// Cascading cache-dir resolution for the lazy-verify sentinel.
//
// Preference order:
//   1. <platform-pkg-dir>/.fallow-verified
//   2. $FALLOW_VERIFY_CACHE_DIR/<package-id>.json
//   3. $XDG_CACHE_HOME/fallow/sentinels/<package-id>.json   (Linux/macOS)
//      or %LOCALAPPDATA%\fallow\sentinels\<package-id>.json (Windows)
//      or ~/.cache/fallow/sentinels/<package-id>.json       (POSIX fallback)
//   4. Every location read-only: returns { path: null, location: 'none', writable: false }.
//      Callers run verify on every invocation and surface FALLOW_SKIP_BINARY_VERIFY=1 as the escape.
//
// Refs RFC 868 (npm/cli#9360). See .plans/rfc-868-lazy-binary-verify.md.

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const SENTINEL_FILENAME = '.fallow-verified';

// Returns true when the directory exists and the current process can create
// a file in it. Tries an atomic O_CREAT|O_EXCL write so we never disturb an
// existing sentinel during the writability probe. Falls back to fs.accessSync
// when mkdtempSync fails for non-permission reasons.
function isWritable(dir) {
  if (typeof dir !== 'string' || dir.length === 0) {
    return false;
  }
  let stat;
  try {
    stat = fs.statSync(dir);
  } catch {
    return false;
  }
  if (!stat.isDirectory()) {
    return false;
  }
  // Probe by creating a unique zero-byte file and deleting it. Cross-platform
  // and tolerant of pnpm/yarn/bun store layouts where the dir is real but the
  // package manager owns its contents.
  const probe = path.join(dir, `.fallow-verify-probe-${process.pid}-${Date.now()}`);
  let fd;
  try {
    fd = fs.openSync(probe, 'wx');
    fs.closeSync(fd);
    fs.unlinkSync(probe);
    return true;
  } catch {
    if (fd !== undefined) {
      try {
        fs.closeSync(fd);
      } catch {}
    }
    return false;
  }
}

function ensureDirExists(dir) {
  try {
    fs.mkdirSync(dir, { recursive: true });
    return true;
  } catch {
    return false;
  }
}

function xdgCacheRoot(env, homeDir, platformId) {
  if (platformId === 'win32') {
    return env.LOCALAPPDATA && env.LOCALAPPDATA.length > 0 ? env.LOCALAPPDATA : null;
  }
  if (env.XDG_CACHE_HOME && env.XDG_CACHE_HOME.length > 0) {
    return env.XDG_CACHE_HOME;
  }
  return homeDir ? path.join(homeDir, '.cache') : null;
}

// Convert an npm package name like "@fallow-cli/darwin-arm64" into a stable
// filesystem-safe identifier used as the sentinel's file basename when the
// sentinel lives outside the platform package directory.
function packageIdToFilename(packageName) {
  if (typeof packageName !== 'string' || packageName.length === 0) {
    return 'unknown.json';
  }
  return `${packageName.replace(/^@/, '').replace(/[\/\\]/g, '__')}.json`;
}

// Resolve the sentinel path according to the cascade documented above.
// Dependency-inject env / homedir / platform / fsProbe so the unit tests can
// exercise every branch without touching the real filesystem state.
//
// Returns: {
//   path: string | null,        // null means "no writable cache location"
//   location: 'platform-pkg' | 'cache-dir-env' | 'xdg' | 'localappdata' | 'home-cache' | 'none',
//   writable: boolean,
// }
function resolveSentinelPath(options) {
  const opts = options || {};
  const platformPkgDir = opts.platformPkgDir;
  const packageName = opts.packageName;
  const env = opts.env || process.env;
  // Using `in` so an explicit `homedir: undefined` opts out of the os.homedir()
  // fallback (tests rely on this to exercise the "no cache home" branch).
  const homeDir = 'homedir' in opts ? opts.homedir : os.homedir();
  const platformId = opts.platform || process.platform;
  const writableProbe = typeof opts.isWritable === 'function' ? opts.isWritable : isWritable;
  const ensureDir = typeof opts.ensureDir === 'function' ? opts.ensureDir : ensureDirExists;

  // Step 1: try the platform pkg dir itself. This is the steady-state path
  // for normal npm / pnpm-hoisted / bun installs.
  if (typeof platformPkgDir === 'string' && platformPkgDir.length > 0) {
    if (writableProbe(platformPkgDir)) {
      return {
        path: path.join(platformPkgDir, SENTINEL_FILENAME),
        location: 'platform-pkg',
        writable: true,
      };
    }
  }

  const filename = packageIdToFilename(packageName);

  // Step 2: explicit override via FALLOW_VERIFY_CACHE_DIR.
  if (env.FALLOW_VERIFY_CACHE_DIR && env.FALLOW_VERIFY_CACHE_DIR.length > 0) {
    const dir = env.FALLOW_VERIFY_CACHE_DIR;
    if (ensureDir(dir) && writableProbe(dir)) {
      return {
        path: path.join(dir, filename),
        location: 'cache-dir-env',
        writable: true,
      };
    }
  }

  // Step 3: XDG / LOCALAPPDATA cache fallback.
  const root = xdgCacheRoot(env, homeDir, platformId);
  if (root) {
    const dir = path.join(root, 'fallow', 'sentinels');
    if (ensureDir(dir) && writableProbe(dir)) {
      return {
        path: path.join(dir, filename),
        location: platformId === 'win32' ? 'localappdata' : env.XDG_CACHE_HOME ? 'xdg' : 'home-cache',
        writable: true,
      };
    }
  }

  return { path: null, location: 'none', writable: false };
}

module.exports = {
  SENTINEL_FILENAME,
  packageIdToFilename,
  resolveSentinelPath,
  // exported for tests
  _isWritable: isWritable,
  _ensureDirExists: ensureDirExists,
  _xdgCacheRoot: xdgCacheRoot,
};
