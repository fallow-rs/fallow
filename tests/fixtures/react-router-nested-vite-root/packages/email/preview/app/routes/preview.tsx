import { Playground } from '../components/playground';

export const loader = () => ({ slug: 'a' });

export default function Preview() {
  return Playground();
}
