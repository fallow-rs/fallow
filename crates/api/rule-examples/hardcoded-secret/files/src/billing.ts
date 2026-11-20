const paymentApiKey = "notReal_a1B2c3D4e5F6g7H8";

export const chargeInvoice = (invoiceId: string) =>
  fetch(`https://payments.internal/invoices/${invoiceId}/charge`, {
    method: "POST",
    headers: { authorization: `Bearer ${paymentApiKey}` },
  });
