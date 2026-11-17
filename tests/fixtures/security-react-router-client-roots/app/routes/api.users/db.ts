import { readFileSync } from 'node:fs'
import { databaseUrl } from '../../lib/db'

export function listUsers() {
	return [databaseUrl, readFileSync('users.json', 'utf8')]
}
