// Real-world consumer pattern: namespace imports from per-component barrels.
// Each `components/ui/<name>/index.ts` is a namespace barrel that exports the
// same short names (Root, Content, Trigger). Without ignoreExports gating
// duplicate-exports, fallow flags these as duplicates even though they are
// intentional API surface.
import * as Dialog from './components/ui/dialog';
import * as Card from './components/ui/card';

export const App = (): unknown => ({
  dialog: Dialog.Root,
  dialogContent: Dialog.Content,
  dialogTrigger: Dialog.Trigger,
  card: Card.Root,
  cardContent: Card.Content,
});
