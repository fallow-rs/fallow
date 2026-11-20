import jwt from "jsonwebtoken";

export const issueCartToken = (cartId: string): string => {
  const payload = { cartId };
  return jwt.sign(payload, "", { algorithm: "none" });
};
