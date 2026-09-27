export async function m1() {
  const { hub } = await import("./hub");
  return hub;
}
