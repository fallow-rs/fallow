export function loader() {
	return { secret: process.env.BANNER_LOADER_SECRET }
}

export default function Banner() {
	return <p>{process.env.BANNER_SECRET}</p>
}
