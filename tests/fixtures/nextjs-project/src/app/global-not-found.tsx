export const metadata = {
  title: 'Missing',
};

export function generateViewport() {
  return { width: 'device-width' };
}

export default function GlobalNotFound() {
  return <html><body>Missing</body></html>;
}

export const unusedGlobalNotFoundHelper = 'still-dead';
