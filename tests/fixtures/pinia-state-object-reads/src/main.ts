import { describePrefs } from './helpers'
import { usePrefsStore } from './stores/prefs'
import { useSessionStore } from './stores/session'
import { useSystemStore } from './stores/system'

const system = useSystemStore()
const { flags } = system.$state
const indexedRead = system.$state['indexed']
// A whole `$state` write replaces state and reads no key.
system.$state = { token: 'next', flags: [], extra: 0, indexed: 0, typedRead: 0, deadState: false }

export const summary = [system.$state.extra, flags, indexedRead, useSystemStore().$state.token]

const prefs = usePrefsStore()
export const label = describePrefs(prefs.$state)

export const sessionCopy = { ...useSessionStore().$state }

type SystemStore = ReturnType<typeof useSystemStore>

export function readTyped(props: { store: SystemStore }): number {
  return props.store.$state.typedRead
}
