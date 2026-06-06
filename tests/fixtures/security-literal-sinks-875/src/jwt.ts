import jwt from "jsonwebtoken";

export function signUnsignedJwt(payload: object): string {
  return jwt.sign(payload, "ignored", { algorithm: "none" });
}
