export interface TypedPair {
  d: number;
}
export const TypedPair = (): TypedPair => ({ d: 4 });
export type InlinePair = { e: number };
export const InlinePair = (): InlinePair => ({ e: 5 });
