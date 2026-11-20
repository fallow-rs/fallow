import AdmZip from "adm-zip";

export const importInvoices = (archivePath: string, targetDir: string): void => {
  const zip = new AdmZip(archivePath);
  zip.extractAllTo(targetDir, true);
};
