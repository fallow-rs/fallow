import { fileURLToPath } from 'node:url';

// Directory target: no trailing slash, no extension. The `services/` directory
// exists but contains NO resolvable module (no index.js), so the specifier is
// genuinely Unresolvable. Because it is extensionless it is marked speculative
// and must be silently dropped: NO unresolved-import finding for `./services`.
const servicesDir = fileURLToPath(new URL('./services', import.meta.url));

// File target that exists with an extension: must resolve normally, no finding.
const workerUrl = new URL('./worker.js', import.meta.url);

// File target with an extension that is genuinely missing: keeps
// is_speculative = false, so it MUST still be reported as unresolved-import.
const missingUrl = new URL('./missing.js', import.meta.url);

export { servicesDir, workerUrl, missingUrl };
