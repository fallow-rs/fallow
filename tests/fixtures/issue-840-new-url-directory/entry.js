import { fileURLToPath } from 'node:url';

// Directory target: no trailing slash, no extension.
// Must NOT produce an unresolved-import finding.
const subDir = fileURLToPath(new URL('./sub', import.meta.url));

// File target that exists: must resolve normally, no finding.
const workerUrl = new URL('./worker.js', import.meta.url);

export { subDir, workerUrl };
