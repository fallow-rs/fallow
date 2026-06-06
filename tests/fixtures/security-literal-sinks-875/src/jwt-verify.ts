import jwt, { verify } from "jsonwebtoken";

export function verifyWithoutAlgorithms(token: string, key: string): unknown {
  return jwt.verify(token, key);
}

export function namedVerifyWithoutAlgorithms(token: string, key: string): unknown {
  return verify(token, key, { audience: "app" });
}

export function verifyWithStaticWrappedOptions(token: string, key: string): unknown {
  return jwt.verify(token, key, { audience: "app" } as const);
}
