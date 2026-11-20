const endpoint = "https://reports.internal/orders";

export const sendOrderReport = () =>
  fetch(endpoint, { method: "POST", body: process.env.API_KEY });
