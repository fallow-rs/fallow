import { createContext } from './context'

const root = createContext()

export const jsxDEV = (tag: string, props: Record<string, unknown>) => ({ tag, props, root })
export const Fragment = (props: { children?: unknown }) => props.children
