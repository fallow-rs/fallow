type ResponseLike = {
  cookie(name: string, value: string, options: Record<string, unknown>): void;
};

export function setSessionCookie(res: ResponseLike): void {
  res.cookie("sid", "value", { sameSite: "lax" });
}
