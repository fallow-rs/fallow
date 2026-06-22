import { LitElement } from 'lit';
import { customElement } from 'lit/decorators.js';

// Mounted via `opts.document.createElement('opt-el')` (a non-`document`
// receiver): credited via the receiver-agnostic createElement capture.
@customElement('opt-el')
class OptEl extends LitElement {}
