// Positive: non-literal browser navigation targets are open-redirect candidates.
export function navigate(target: string): void {
  location.href = target;
  location.assign(target);
  window.open(target);
}
