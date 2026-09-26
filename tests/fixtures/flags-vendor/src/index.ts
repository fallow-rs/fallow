import { checkout } from "./checkout";

export function boot(): string {
  if (variation("new-checkout", false)) {
    return checkout();
  }
  if (variation("old-banner", false)) {
    return "banner";
  }
  if (variation("beta-typo", false)) {
    return "beta";
  }
  if (checkGate("statsig-gate")) {
    return "gate";
  }
  return variation("live-experiment", false) ? "a" : "b";
}
