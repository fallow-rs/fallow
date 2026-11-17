import type { Invoice } from "../shared";

export const shipTypeViaShared = (invoice: Invoice): number => invoice.id;
