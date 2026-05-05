// Side-effect imports trigger the registration in each module.
// Nobody references the class identifiers directly.
import './my-element.js';
import './other-element.js';
import './separate-define.js';
import './named-alias-decorator.js';
import './named-import-alias-decorator.js';
import './anonymous-default.js';

const root = document.body;
const tags = [
  'my-element',
  'other-element',
  'separate-element',
  'aliased-element',
  'named-import-aliased-element',
  'anonymous-default-element',
];
for (const tag of tags) {
  root.appendChild(document.createElement(tag));
}
