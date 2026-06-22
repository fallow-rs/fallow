import { LitElement } from 'lit';
import { customElement } from 'lit/decorators.js';

// Rendered only as `<html-el>` in the standalone index.html app shell, never in
// an html`` template: credited via the .html custom-element scan.
@customElement('html-el')
class HtmlEl extends LitElement {}
