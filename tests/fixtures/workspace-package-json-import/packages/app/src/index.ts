import libManifest from '@fx/lib/package.json' with { type: 'json' };
import plainManifest from '@fx/plain/package.json' with { type: 'json' };
import blockedManifest from '@fx/blocked/package.json' with { type: 'json' };

export const versions = [libManifest.version, plainManifest.version, blockedManifest.version];
