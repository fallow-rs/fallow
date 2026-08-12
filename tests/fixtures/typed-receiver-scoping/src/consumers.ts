import type {
  AliasContext,
  FirstContext,
  NestedContext,
  SecondContext,
} from './contexts'

export type ContextSurface = Pick<AliasContext, 'aliasUsed' | 'pickedOnly'> & {
  nested: NestedContext
}

export function useFirst(ctx: FirstContext): void {
  ctx.firstUsed()
}

export function useSecond(ctx: SecondContext): void {
  ctx.secondUsed()
}

export function useAlias(ctx: ContextSurface): void {
  ctx.aliasUsed()
}
