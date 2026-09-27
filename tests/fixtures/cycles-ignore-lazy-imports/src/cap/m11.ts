export async function m11() {
  const { hub } = await import("./hub");
  return hub;
}
