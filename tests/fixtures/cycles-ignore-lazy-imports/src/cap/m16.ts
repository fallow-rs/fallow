export async function m16() {
  const { hub } = await import("./hub");
  return hub;
}
