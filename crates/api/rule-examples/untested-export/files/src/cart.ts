import { formatPrice, roundCents } from "./money";

const total = roundCents(19.999);
console.log(formatPrice(total));
