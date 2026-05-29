import { LitElement, html } from 'lit';
import { customElement } from 'lit/decorators.js';

// Lit element living ONLY in a workspace package. The root project does not
// depend on lit, so the Lit plugin is active for this package alone. Its
// heritage-scoped class-member allowlist must survive the workspace-result
// merge (issue #772), otherwise render() below is wrongly flagged as
// unused-class-member.
@customElement('workspace-element')
export class WorkspaceElement extends LitElement {
  render() {
    return html`<p>hello from a workspace package</p>`;
  }

  firstUpdated() {
    // Lit-specific lifecycle method, called reflectively by the framework.
    // Unlike render(), firstUpdated is NOT in fallow's built-in React/native
    // lifecycle lists, so crediting it depends ENTIRELY on the Lit plugin's
    // heritage-scoped allowlist surviving the workspace merge (issue #772).
    this.requestUpdate();
  }

  unusedHelper() {
    // Genuinely unused, non-lifecycle method. The merge fix must not suppress
    // real findings: this should STILL be reported as unused-class-member,
    // proving the detector ran (non-vacuous control).
    return 'never called';
  }
}
