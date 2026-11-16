import { track } from '../lib/tracker.client'
import { save } from '../.client/storage'

export default function Widget() {
	return <button onClick={() => { track(); save() }}>go</button>
}
