import type { FirstContext, SecondContext } from './contexts'

export function useFirst(ctx: FirstContext): void {
  ctx.firstUsed()
}

export function useSecond(ctx: SecondContext): void {
  ctx.secondUsed()
}
