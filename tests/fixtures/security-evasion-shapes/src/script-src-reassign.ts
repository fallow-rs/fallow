export const load = (src: string, script?: HTMLScriptElement): void => {
  script = document.createElement("script");
  script.src = src;
  document.body.appendChild(script);
};
