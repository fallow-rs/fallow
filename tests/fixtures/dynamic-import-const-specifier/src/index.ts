import { loadLocal } from './local'

const modulePackage = 'module-scope-pkg'

export const loadModuleScope = () => import(modulePackage)

export const loadFunctionScope = async () => {
  const functionPackage = 'function-scope-pkg'
  const { thing } = await import(/* @vite-ignore */ functionPackage)
  return thing
}

export const loadSubpath = async () => {
  const handlerPath = `subpath-pkg/lib/handler`
  const { createHandler } = await import(handlerPath)
  return createHandler
}

const shadowedPackage = 'shadowed-pkg'
export const loadShadowed = (shadowedPackage: string) => import(shadowedPackage)

export const loadLet = () => {
  let letPackage = 'let-pkg'
  letPackage = `${letPackage}-other`
  return import(letPackage)
}

export { loadLocal, shadowedPackage }

export const declareSibling = () => {
  const siblingPackage = 'sibling-pkg'
  return siblingPackage.length
}

export const loadSibling = (name: string) => {
  const load = (siblingPackage: string) => import(siblingPackage)
  return load(name)
}

const caughtPackage = 'caught-pkg'
export const loadCaught = async () => {
  try {
    return await import('./target')
  } catch (caughtPackage) {
    return import(caughtPackage as string)
  }
}
export { caughtPackage }

export const loadNestedCaught = async () => {
  const nestedCaught = 'nested-caught-pkg'
  try {
    return await import('./target')
  } catch (nestedCaught) {
    return import(nestedCaught as string)
  }
}

export const loadHoisted = (value: string) => {
  const hoisted = 'hoisted-pkg'
  const inner = () => {
    const loaded = import(hoisted)
    var hoisted = value
    return loaded
  }
  return inner()
}

export const loadLoop = (names: string[]) => {
  const looped = 'loop-pkg'
  for (const looped of names) {
    void import(looped)
  }
}
