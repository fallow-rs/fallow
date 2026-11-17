import { index, route } from '@react-router/dev/routes'

export default [
	index('routes/home.tsx'),
	route('admin', 'routes/admin.tsx'),
	route('session', 'routes/session.tsx'),
	route('widget', 'routes/widget.tsx'),
]
