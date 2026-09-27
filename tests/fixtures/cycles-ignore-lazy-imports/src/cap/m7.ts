export async function m7() {
  const { hub } = await import("./hub");
  return hub;
}
