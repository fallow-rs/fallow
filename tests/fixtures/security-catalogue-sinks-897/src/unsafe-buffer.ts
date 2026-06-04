// Positive: Buffer.allocUnsafe / allocUnsafeSlow return uninitialized memory
// (CWE-1188); a non-literal length is captured.
export function makeBuffer(size: number): Buffer {
  return Buffer.allocUnsafe(size);
}

export function makeSlowBuffer(size: number): Buffer {
  return Buffer.allocUnsafeSlow(size);
}
