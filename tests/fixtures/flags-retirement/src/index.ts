import { checkout } from "./checkout";

export function boot(): string {
  if (process.env.FEATURE_SINGLE) {
    return checkout();
  }
  if (process.env.FEATURE_WIDE) {
    return "wide";
  }
  return "default";
}

export { Banner, price } from "./branches";
export { killed } from "./constants";
