import { loadGauge, makePending, makePlain, makeWidget } from './lib.ts';
import type { Gauge, Pending, Plain, Widget } from './lib.ts';

export const keep: [Widget?, Gauge?, Plain?, Pending?] = [];

const widget = await makeWidget();
widget.usedAtTopLevel();

export async function run(): Promise<void> {
  const inner = await makeWidget();
  inner.usedInAsyncFunction();
  const gauge = await loadGauge();
  gauge.readValue();
  const plain = await makePlain();
  plain.readAfterAwait();
}

const pending = makePending();
pending.onlyReadOnPromise();
