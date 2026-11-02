export const label = (text: string, src: string): void => {
  const el = document.createElement("span");
  el.textContent = text;
  const img = document.createElement("img");
  img.src = src;
  document.body.append(el, img);
};
