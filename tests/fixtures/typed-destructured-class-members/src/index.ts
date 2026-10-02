import { Client, Service, Store } from './lib';

export function a(input: { service: Service }) {
  const { service } = input;
  service.used();
}

export function b(o: { client: Client }) {
  const { client: renamed } = o;
  renamed.used();
}

interface Holder {
  store: Store;
}

export function c(holder: Holder) {
  const { store } = holder;
  store.used();
}
