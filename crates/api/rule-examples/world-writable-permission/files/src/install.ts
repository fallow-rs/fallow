import fs from "node:fs";

export const installInvoiceScript = (scriptPath: string, source: string) => {
  fs.writeFileSync(scriptPath, source);
  fs.chmodSync(scriptPath, 0o777);
};
