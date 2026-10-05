import unlistedReq from 'unlisted-req';
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
export const both = [unlistedReq, require.resolve('unlisted-req')];
