import crypto from "node:crypto";

export const encryptInvoice = (invoice: string, password: string): string => {
  const cipher = crypto.createCipher("aes-256-cbc", password);
  return cipher.update(invoice, "utf8", "hex") + cipher.final("hex");
};
