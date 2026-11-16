import { readFileSync } from 'node:fs'

export function readNote() {
	return readFileSync('note.txt', 'utf8')
}
