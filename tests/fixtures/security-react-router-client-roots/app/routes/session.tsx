import { currentUser } from '../lib/auth.server'

export async function loader() {
	return currentUser()
}

export function meta() {
	return [{ title: String(currentUser().id) }]
}

export default function Session() {
	return <p>session</p>
}
