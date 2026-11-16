import { stripeKey } from '../lib/billing'
import { readNote } from '../lib/files'

export default function Home() {
	return <p>{stripeKey} {readNote()}</p>
}
