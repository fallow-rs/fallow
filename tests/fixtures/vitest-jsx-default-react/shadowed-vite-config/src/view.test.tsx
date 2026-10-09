import { expect, test } from 'vitest'

test('renders a view', () => {
  expect(<div title="view" />).toBeTruthy()
})
