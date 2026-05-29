// Side-effect import registers the custom element at module load time.
// Nobody references WorkspaceElement by name, so its reachability comes from
// this import + the @customElement registration.
import './workspace-element.js';

const root = document.body;
root.appendChild(document.createElement('workspace-element'));
