import { mixedHelper } from "./a";
export async function mixedB() {
  const { mixedLater } = await import("./a");
  return mixedHelper() + mixedLater();
}
