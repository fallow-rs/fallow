export async function m5() {
  const { hub } = await import("./hub");
  return hub;
}
