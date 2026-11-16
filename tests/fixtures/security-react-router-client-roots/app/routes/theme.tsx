import { readFileSync } from 'node:fs'
import { setTheme } from '../lib/theme.server'

export async function action() {
	const fallback = readFileSync('theme.txt', 'utf8')
	return setTheme(process.env.THEME_SECRET ?? fallback)
}

export function ThemeSwitch() {
	return <button>theme</button>
}
