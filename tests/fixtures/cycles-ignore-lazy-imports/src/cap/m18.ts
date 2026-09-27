export async function m18() {
  const { hub } = await import("./hub");
  return hub;
}
