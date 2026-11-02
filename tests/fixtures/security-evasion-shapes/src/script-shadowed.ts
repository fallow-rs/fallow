const el = document.createElement("script");
export const set = (src: string): void => {
  const el = document.createElement("img");
  el.src = src;
};
export const unrelated = (el: HTMLImageElement, src: string): void => {
  el.src = src;
};
export const top = (code: string): void => {
  el.textContent = code;
};
