export async function generateMetadata() {
  return { title: 'Dynamic not found' };
}

export function generateViewport() {
  return { width: 'device-width' };
}

export default function DynamicNotFound() {
  return <div>Dynamic Not Found</div>;
}

export const unusedDynamicNotFoundHelper = 'still-dead';
