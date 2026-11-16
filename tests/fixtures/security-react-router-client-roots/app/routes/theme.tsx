import { setTheme } from '../lib/theme.server'

export async function action() {
	return setTheme('dark')
}

export function ThemeSwitch() {
	return <button>theme</button>
}
