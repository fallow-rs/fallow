export const run = (frame: HTMLIFrameElement, html: string): void => frame.setAttribute("srcdoc", html);
