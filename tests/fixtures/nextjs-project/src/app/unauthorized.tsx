export async function generateMetadata() {
  return { title: 'Unauthorized' };
}

export function generateViewport() {
  return { width: 'device-width' };
}

export default function Unauthorized() {
  return <div>Unauthorized</div>;
}

export const unusedUnauthorizedHelper = 'still-dead';
