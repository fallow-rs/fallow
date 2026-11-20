import crypto from "node:crypto";

export interface ChecksumOptions {
  algorithm: string;
}

export const invoiceChecksum = (invoice: string, options: ChecksumOptions): string =>
  crypto.createHash(options.algorithm).update(invoice).digest("hex");
