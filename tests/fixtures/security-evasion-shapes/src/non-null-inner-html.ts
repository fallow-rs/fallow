export const render = (el: HTMLElement | null, markup: string): void => {
  el!.innerHTML = markup;
};
