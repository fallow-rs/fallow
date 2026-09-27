export async function m22() {
  const { hub } = await import("./hub");
  return hub;
}
