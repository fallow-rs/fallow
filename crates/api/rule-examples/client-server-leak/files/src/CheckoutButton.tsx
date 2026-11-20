"use client";

import { createPayment } from "./payments";

export const CheckoutButton = () => (
  <button onClick={() => createPayment(4200)}>Pay now</button>
);
