import { PartnerFactory } from "./partner";

export const readGuards = (): void => {
  // @ts-expect-error The async factory gives a promise, not a PartnerFactory.
  PartnerFactory.fromAsync("c").asyncOnly();
  PartnerFactory.fromFlag("d", false)?.flagOnly();
};
