export async function m2() {
  const { hub } = await import("./hub");
  return hub;
}
