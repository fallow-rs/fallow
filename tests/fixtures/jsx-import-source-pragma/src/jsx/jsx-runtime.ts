import { createContext } from './context'

const root = createContext()

export const jsx = (tag: string, props: Record<string, unknown>) => ({ tag, props, root })
export const jsxs = jsx
export const Fragment = (props: { children?: unknown }) => props.children
