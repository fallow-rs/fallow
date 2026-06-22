import { LitElement } from 'lit';

// Registered via the `window.`-qualified registry call and rendered nowhere:
// flagged, which proves `window.customElements.define` is captured as a
// registration (a bare-`customElements`-only gate would miss it and produce no
// finding at all).
class WinEl extends LitElement {}
window.customElements.define('win-el', WinEl);
