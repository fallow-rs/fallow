export async function m8() {
  const { hub } = await import("./hub");
  return hub;
}
