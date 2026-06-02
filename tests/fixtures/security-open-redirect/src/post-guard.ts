interface Res {
  redirect(target: string): void;
}

const ALLOWED_REDIRECTS = ["/dashboard"];

export function handle(res: Res, userTarget: string): void {
  res.redirect(userTarget);

  if (!ALLOWED_REDIRECTS.includes(userTarget)) {
    throw new Error("redirect target not allowed");
  }
}
