export async function m10() {
  const { hub } = await import("./hub");
  return hub;
}
