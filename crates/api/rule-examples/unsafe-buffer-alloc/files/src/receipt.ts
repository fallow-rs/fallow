export const encodeReceipt = (lines: string[], size: number): Buffer => {
  const buffer = Buffer.allocUnsafe(size);
  let offset = 0;
  for (const line of lines) {
    offset += buffer.write(`${line}\n`, offset);
  }
  return buffer;
};
