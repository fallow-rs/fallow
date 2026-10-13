export const revalidate = 60 * 5;
export const dynamic = "force-static";

export function heading(count: number): string {
  return count > 1 ? "posts" : "post";
}

export default function Page(): string {
  return heading(revalidate / 60);
}
