import type { Invoice } from "../billing/types";

export const shipTypeDeep = (invoice: Invoice): number => invoice.id;
