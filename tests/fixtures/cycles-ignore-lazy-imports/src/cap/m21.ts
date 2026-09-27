export async function m21() {
  const { hub } = await import("./hub");
  return hub;
}
