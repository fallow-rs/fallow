import { createInvoice } from "../billing/public";

export const viaPublic = (): number => createInvoice();
