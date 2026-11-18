import { createInvoice } from "../billing/invoice";

export const direct = (): number => createInvoice();
