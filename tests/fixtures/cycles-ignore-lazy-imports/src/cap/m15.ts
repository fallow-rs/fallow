export async function m15() {
  const { hub } = await import("./hub");
  return hub;
}
