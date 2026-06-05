import { dirname, join } from "node:path";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);

const CANONICAL_FONTS = {
  inter: {
    packageName: "@fontsource/inter",
    faces: [{ weight: "400" }],
  },
};

function packageRoot(packageName: string): string {
  const packageJsonPath = require.resolve(`${packageName}/package.json`);
  return dirname(packageJsonPath);
}

function resolveFontFile(packageName: string, weight: string): string {
  return join(packageRoot(packageName), "files", `${weight}.woff2`);
}

function resolveModuleDir(moduleName: string): string {
  return join("node_modules", moduleName);
}

for (const [, font] of Object.entries(CANONICAL_FONTS)) {
  resolveFontFile(font.packageName, font.faces[0].weight);
}

join(resolveModuleDir("ffmpeg-static"), "ffmpeg");
join(resolveModuleDir("ffprobe-static"), "bin/linux/x64/ffprobe");
