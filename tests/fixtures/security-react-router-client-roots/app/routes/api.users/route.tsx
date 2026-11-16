import { listUsers } from './db'

export function loader() {
	return listUsers()
}
