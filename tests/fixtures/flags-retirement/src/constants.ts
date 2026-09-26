const FEATURE_KILL_SWITCH = false;

export function killed(): string {
  return FEATURE_KILL_SWITCH ? "off" : "on";
}
