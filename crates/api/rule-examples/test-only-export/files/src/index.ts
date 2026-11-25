import { formatPrice } from "./price";

export const label = (cents: number): string => `Total: ${formatPrice(cents)}`;
