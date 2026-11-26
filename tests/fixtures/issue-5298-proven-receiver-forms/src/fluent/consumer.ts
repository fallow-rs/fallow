import { PartnerFactory } from "./partner";

export const created = PartnerFactory.create("a");
export const chained = PartnerFactory.from("b").chained().build();
