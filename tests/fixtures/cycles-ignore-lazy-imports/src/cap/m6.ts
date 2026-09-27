export async function m6() {
  const { hub } = await import("./hub");
  return hub;
}
