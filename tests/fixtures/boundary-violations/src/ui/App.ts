// Allowed: ui -> shared
import { helper } from '../shared/utils';

// Violation: ui -> db (not in allow list)
import { query } from '../db/query';
import { generatedClient } from '../generated/client';

export const app = () => helper() + query() + generatedClient();

// Control: ui zone has no forbidden-call rule, so this stays quiet.
console.log('boot');
