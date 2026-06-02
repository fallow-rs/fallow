interface Res {
  redirect(target: string): void;
}

const ALLOWED_REDIRECTS = new Set(["/dashboard", "/settings"]);

export function handle(res: Res, userTarget: string): void {
  if (!ALLOWED_REDIRECTS.has(userTarget)) {
    throw new Error("redirect target not allowed");
  }

  res.redirect(userTarget);
}
