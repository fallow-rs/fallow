import type { PrefsState } from './stores/prefs'

export function describePrefs(state: PrefsState): string {
  return `${state.theme}/${state.locale}`
}
