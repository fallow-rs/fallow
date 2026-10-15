import { lazy as component } from 'react';
import loadable from '@loadable/component';
declare function awaitPreservingAbortReason<T>(operation: () => Promise<T>): Promise<T>;
const loadNamed = async () => await import('./named');
const loadPrecise = () => import('./precise');
const lazy = <T>(operation: () => Promise<T>) => operation();
export const View = component(() => import('./view'));
export const NamedView = loadable(() => import('./configurable'), { resolveComponent: mod => mod.used });

export async function run() {
  const { used } = await awaitPreservingAbortReason(async () => await import('./inline'));
  used();
  const named = await awaitPreservingAbortReason(loadNamed);
  named.used();
  const local = await lazy(() => import('./lookalike'));
  local.used();
  const precise = await loadPrecise();
  precise.used();
  const unreadPromise = import('./unread');
  void import('./discarded');
}
export function shadow(component: (operation: () => Promise<unknown>) => unknown) {
  return component(() => import('./shadowed'));
}

namespace Local {
  const component = <T>(load: () => Promise<T>) => load();
  export const result = component(() => import('./local-namespace'));
}
export namespace Exported {
  const component = <T>(load: () => Promise<T>) => load();
  export const result = component(() => import('./exported-namespace'));
}
try {} catch (component) {
  component(() => import('./caught'));
}
