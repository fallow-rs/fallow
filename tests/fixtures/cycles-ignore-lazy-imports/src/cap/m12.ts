export async function m12() {
  const { hub } = await import("./hub");
  return hub;
}
