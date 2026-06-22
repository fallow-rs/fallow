import { LitElement } from 'lit';
import { customElement } from 'lit/decorators.js';

// Registered for side effect but rendered in NO html`` template: unrendered.
@customElement('dead-el')
class DeadEl extends LitElement {}
