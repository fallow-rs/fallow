export const inline = (code: string): void => {
  const el = document.createElement("script");
  el.textContent = code;
  document.head.appendChild(el);
};
