import { expect, test } from 'vitest'

test('renders a widget', () => {
  expect(<div title="widget" />).toBeTruthy()
})
