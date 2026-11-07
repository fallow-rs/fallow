export function settle(n: number): number {
  return n;
}

export async function pause(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 1));
}

export function shout(text: string): string {
  return text.toUpperCase();
}

export function spin(limit: number): number {
  let count = 0;
  while (count < limit) {
    count = count + 1;
  }
  return count;
}

export const READY = true;
