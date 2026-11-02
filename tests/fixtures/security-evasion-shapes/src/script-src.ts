export const load = (src: string): void => {
  const el = document.createElement("script");
  el.src = src;
  document.head.appendChild(el);
};
