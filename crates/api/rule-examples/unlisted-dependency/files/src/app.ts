import { formatPrice } from "@acme/shared";

export const cartLabel = (total: number): string => `Total: ${formatPrice(total)}`;
