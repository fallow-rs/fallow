interface Res {
  redirect(target: string): void;
}

const ALLOWED_REDIRECTS = new Set(["/dashboard"]);

export function handle(res: Res, userTarget: string): void {
  ALLOWED_REDIRECTS.add(userTarget);

  if (!ALLOWED_REDIRECTS.has(userTarget)) {
    throw new Error("redirect target not allowed");
  }

  res.redirect(userTarget);
}
