import { header, footer } from './Layout.module.css';
import styles from './Button.module.css';

const clsx = (...names: string[]) => names.join(' ');
const { primary } = styles;

export const app = clsx(header, footer, primary);
