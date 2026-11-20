export const createPayment = (amount: number) =>
  fetch("https://payments.internal/charge", {
    method: "POST",
    headers: { authorization: `Bearer ${process.env.PAYMENT_API_KEY}` },
    body: JSON.stringify({ amount }),
  });
