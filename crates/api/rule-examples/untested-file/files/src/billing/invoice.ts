export const createInvoice = (orderId: string, total: number): string =>
  `Invoice for ${orderId}: ${total.toFixed(2)}`;
