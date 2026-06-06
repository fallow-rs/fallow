import jwt, { verify } from "jsonwebtoken";

export function verifyWithoutAlgorithms(token: string, key: string): unknown {
  return jwt.verify(token, key);
}

export function namedVerifyWithoutAlgorithms(token: string, key: string): unknown {
  return verify(token, key, { audience: "app" });
}
