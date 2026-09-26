import { flag } from "flags/next";

export const showSale = flag({ key: "summer-sale", decide: () => false });
export const showLegacy = flag({ key: "legacy-banner", decide: () => false });
