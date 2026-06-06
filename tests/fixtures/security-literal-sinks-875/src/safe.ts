import cors from "cors";
import * as crypto from "node:crypto";
import jwt from "jsonwebtoken";

type ResponseLike = {
  cookie(name: string, value: string, options: Record<string, unknown>): void;
};

declare const res: ResponseLike;

function refresh(): void {}

export const middleware = cors({
  origin: "https://example.com",
  credentials: true,
});

export function safeForms(): void {
  window.parent.postMessage({ status: "ready" }, "https://example.com");
  res.cookie("sid", "value", { httpOnly: true, secure: true });
  crypto.createHash("sha256").update("value").digest("hex");
  setTimeout(() => refresh(), 1000);
  const previewNumber = Math.random();
  jwt.sign({ sub: "1" }, "ignored", { algorithm: "HS256" });
  void previewNumber;
  void fetch("https://example.com/status");
}
