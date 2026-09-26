export function checkout(): string {
  return variation("new-checkout", false) ? "new" : "old";
}
