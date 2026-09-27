export async function m20() {
  const { hub } = await import("./hub");
  return hub;
}
