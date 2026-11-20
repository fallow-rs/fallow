import fs from "node:fs";

export const saveOrderReport = (data: string) => {
  fs.writeFileSync("/tmp/report.json", data);
};
