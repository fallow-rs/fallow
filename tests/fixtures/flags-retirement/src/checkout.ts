export function checkout(): string {
  return process.env.FEATURE_WIDE ? "new" : "old";
}
