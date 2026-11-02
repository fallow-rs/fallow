export const render = (el: unknown, markup: string): void => {
  (el as HTMLElement).innerHTML = markup;
};
