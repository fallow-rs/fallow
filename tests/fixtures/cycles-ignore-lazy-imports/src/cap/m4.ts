export async function m4() {
  const { hub } = await import("./hub");
  return hub;
}
