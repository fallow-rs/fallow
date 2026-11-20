import { expect, it } from "vitest";
import { createInvoice } from "../src/billing/invoice";

it("formats the total", () => {
  expect(createInvoice("order-1", 40)).toBe("Invoice for order-1: 40.00");
});
