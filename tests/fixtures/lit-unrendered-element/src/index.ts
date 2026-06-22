// App entry: side-effect imports register the elements; the root is mounted
// imperatively (createElement credits `my-app` as rendered).
import './my-app';
import './dead-el';
import './html-el';
import './opt-el';
import './docs/docs-el';
import './win-el';

document.body.appendChild(document.createElement('my-app'));

// A non-`document` receiver (`opts.document`) still credits the tag via the
// receiver-agnostic createElement capture.
function mountOpt(opts: { document: Document }): void {
  opts.document.body.appendChild(opts.document.createElement('opt-el'));
}
mountOpt({ document });
