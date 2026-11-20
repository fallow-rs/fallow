import jwt from "jsonwebtoken";

const publicKey = process.env.JWT_PUBLIC_KEY ?? "";

export const readUserId = (token: string): string => {
  const claims = jwt.verify(token, publicKey, { audience: "api" });
  return typeof claims === "string" ? claims : String(claims.sub);
};
