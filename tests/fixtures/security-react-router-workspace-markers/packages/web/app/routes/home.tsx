import { track } from '../tracker.client'

export default function Home() {
	return <button onClick={() => track()}>home</button>
}
