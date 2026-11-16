import { readFileSync } from 'node:fs'
import { databaseUrl } from '../lib/db'

export function loader() {
	return {
		connected: Boolean(databaseUrl),
		note: readFileSync('note.txt', 'utf8'),
		token: process.env.ADMIN_SECRET,
	}
}

export default function Admin() {
	return <p>admin</p>
}
