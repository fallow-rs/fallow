declare const client: { isEnabled(name: string): boolean };

export const unconfirmed = client.isEnabled("ext-test");
export const confirmed = checkGate("statsig-gate");
