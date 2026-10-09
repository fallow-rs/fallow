/** @jsxImportSource ./jsx */
import { Note, describeNote } from './doc-comment'
import { label } from './no-jsx'
import { Widget } from './widget'

export const App = () => (
  <main title={`${label} ${describeNote()}`}>
    <Widget />
    <Note />
  </main>
)
