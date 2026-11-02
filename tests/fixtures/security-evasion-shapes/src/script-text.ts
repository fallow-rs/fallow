export const inline = (code: string): void => {
  const el = document.createElement("script");
  el.text = code;
};
