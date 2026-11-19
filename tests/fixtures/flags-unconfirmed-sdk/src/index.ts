declare const client: { isEnabled(name: string): boolean };

export const unconfirmed = client.isEnabled("ext-test");
export const confirmed = checkGate("statsig-gate");

declare const flags: { isEnabled(name: string): boolean; isOn(name: string): boolean };

export const first = flags.isEnabled("dup-flag");
export const second = flags.isOn("dup-flag");
