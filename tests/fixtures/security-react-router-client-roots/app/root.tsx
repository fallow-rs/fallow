import { Outlet } from 'react-router'
import { sessionSecret } from './lib/session'
import { analyticsToken } from './lib/analytics'
import { readNote } from './lib/files'

export async function loader() {
	return { signed: Boolean(sessionSecret), note: readNote() }
}

export function Layout({ children }: { children: unknown }) {
	return <html data-token={analyticsToken}>{children}</html>
}

export default function App() {
	return <Outlet />
}
