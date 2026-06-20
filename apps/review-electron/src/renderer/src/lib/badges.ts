import type { WalkthroughFile } from "../../../model/walkthrough";

export type Tone = "high" | "info" | "muted";
export type Badge = { label: string; tone: Tone };

const HIGH_FAN_IN = 4;

/** Deterministic per-file badges derived purely from the Fallow focus score. */
export const deriveFileBadges = (file: WalkthroughFile): Badge[] => {
  const badges: Badge[] = [
    file.deprioritized
      ? { label: "DEPRIORITIZED", tone: "muted" }
      : { label: "REVIEW HERE", tone: "high" },
  ];
  if (file.score.fanIo >= HIGH_FAN_IN) badges.push({ label: "HIGH FAN-IN", tone: "info" });
  if (file.score.securityTaint > 0) badges.push({ label: "SECURITY", tone: "high" });
  if (file.score.riskZone > 0) badges.push({ label: "RISK ZONE", tone: "info" });
  return badges;
};
