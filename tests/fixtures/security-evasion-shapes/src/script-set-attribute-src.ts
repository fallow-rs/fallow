export const load = (src: string): void => {
  const el = document.createElement("script");
  el.setAttribute("src", src);
  document.head.appendChild(el);
};
