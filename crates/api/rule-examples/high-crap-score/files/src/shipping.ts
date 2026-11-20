export const shippingCost = (country: string, weight: number): number => {
  switch (country) {
    case "NL":
      return weight > 10 ? 9 : 5;
    case "BE":
      return 7;
    case "DE":
      return 8;
    case "FR":
      return 10;
    default:
      return 25;
  }
};
