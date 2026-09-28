const shimSource = require('!raw-loader?esModule=false!./sandbox/shim.js');
import styles from '!!style-loader!css-loader?modules!./styles.css';
import Worker from '-!@example/worker-loader?inline=true!./worker.js';

export const loadTemplate = () => import('raw-loader!./sandbox/template.js?inline');

export const run = () => [shimSource, styles, new Worker()];
