export const route = (path: string): string => {
  switch (path) {
    case "/cart":
      return "cart";
    case "/checkout":
      return "checkout";
    case "/orders":
      return "orders";
    case "/invoices":
      return "invoices";
    case "/account":
      return "account";
    default:
      return "not-found";
  }
};
