import https from "node:https";

export const fetchOrders = () =>
  https.request("https://orders.internal/api/orders", { rejectUnauthorized: false }, (response) => {
    response.pipe(process.stdout);
  });
