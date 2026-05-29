// rspress exposes its theme layer through the `@theme` build-time virtual
// module. Both the bare `@theme` import and a `@theme/<component>` subpath are
// resolved by the rspress bundler, not by npm, so neither should report as an
// unlisted dependency or an unresolved import.
import Theme from '@theme';
import { Layout } from '@theme/Layout';

// Control: a genuinely-missing bare package MUST still report as unlisted, so
// the regression test is non-vacuous and proves dependency detection ran.
import { thing } from 'definitely-missing-pkg';

export function Page() {
  return Theme(Layout(thing));
}
