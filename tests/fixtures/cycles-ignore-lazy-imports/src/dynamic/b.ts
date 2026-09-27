export async function dynamicB() {
  const { dynamicHelper } = await import("./a");
  return dynamicHelper();
}
