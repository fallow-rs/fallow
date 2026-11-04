export const ICON_ROOT = new URL("./node_modules/icon-pack/svgs/", import.meta.url).pathname;
export const LOGO = new URL("./node_modules/@scope/assets/logo.svg", import.meta.url).pathname;
export const iconUrl = (name: string): URL =>
  new URL(`./node_modules/tpl-icons/svgs/${name}.svg`, import.meta.url);
