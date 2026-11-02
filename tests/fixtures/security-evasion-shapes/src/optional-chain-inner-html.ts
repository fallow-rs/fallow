export const render = (root: { panel?: { body: HTMLElement } }, markup: string): void => {
  (root.panel?.body as HTMLElement).innerHTML = markup;
};
