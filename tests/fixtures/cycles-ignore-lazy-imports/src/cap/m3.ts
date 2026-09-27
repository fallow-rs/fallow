export async function m3() {
  const { hub } = await import("./hub");
  return hub;
}
