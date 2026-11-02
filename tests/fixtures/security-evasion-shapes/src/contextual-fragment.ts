export const parse = (icon: string): DocumentFragment =>
  document.createRange().createContextualFragment(icon);
