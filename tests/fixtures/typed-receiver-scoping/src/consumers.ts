import type {
  AliasContext,
  FirstContext,
  InnerHandlerContext,
  NestedContext,
  OuterHandlerContext,
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

type Handler = (context: OuterHandlerContext) => void

export function createHandler() {
  const handler: Handler = context => context.innerUsed()
  type Handler = (context: InnerHandlerContext) => void
  return handler
}
