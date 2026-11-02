export const run = (el: HTMLElement, prefix: string, html: string): void => { (el as unknown as Record<string, string>)[prefix + "HTML"] = html; };
