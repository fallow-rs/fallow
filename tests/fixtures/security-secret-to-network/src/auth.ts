// A non-public env secret reaching a network call body with a LITERAL
// destination: usually intended auth (the credential's own provider).
const key = process.env.STRIPE_SECRET_KEY;
export async function charge() {
  await fetch("https://api.stripe.com/v1/charges", {
    method: "POST",
    headers: { authorization: `Bearer ${key}` },
  });
}
