export async function m24() {
  const { hub } = await import("./hub");
  return hub;
}
