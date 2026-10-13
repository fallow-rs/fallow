export function settle(n: number): number {
  return n;
}

export async function pause(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 1));
}

export function shout(text: string): string {
  return text.toUpperCase();
}

export const READY = true;
