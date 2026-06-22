import { LitElement } from 'lit';
import { customElement } from 'lit/decorators.js';

// Defined under a `docs/` directory: rendered (if at all) by docs-site tooling
// fallow cannot parse, so the arm abstains rather than risk a false positive,
// even though this element is rendered in no html`` template.
@customElement('docs-el')
class DocsEl extends LitElement {}
