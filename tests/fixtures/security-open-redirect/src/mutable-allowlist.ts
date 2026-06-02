interface Res {
  redirect(target: string): void;
}

let allowedRedirects = new Set(["/dashboard"]);

export function handle(res: Res, userTarget: string): void {
  if (!allowedRedirects.has(userTarget)) {
    throw new Error("redirect target not allowed");
  }

  res.redirect(userTarget);
}
