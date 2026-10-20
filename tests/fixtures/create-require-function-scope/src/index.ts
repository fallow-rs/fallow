import { createRequire } from 'node:module';
import * as nodeModule from 'node:module';

export function a() {
  const require = createRequire(import.meta.url);
  return require.resolve('pkg-a');
}
export async function b() {
  const require = createRequire(import.meta.url);
  return { x: require.resolve('pkg-b') };
}
export function c() {
  const require = createRequire(import.meta.url);
  return require('pkg-c');
}
export function d() {
  const require = nodeModule.createRequire(import.meta.url);
  return require.resolve('pkg-d');
}
export function e(require: NodeRequire) {
  return require.resolve('pkg-e');
}
