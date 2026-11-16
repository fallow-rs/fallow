import { Outlet } from 'react-router'

export { saveNote as action } from '../lib/notes.server'

export default function Notes() {
	return <Outlet />
}
