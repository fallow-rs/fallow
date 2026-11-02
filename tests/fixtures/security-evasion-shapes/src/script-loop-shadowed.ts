const s = document.createElement("script");
export const ofLoop = (list: HTMLImageElement[], src: string): void => {
  for (const s of list) {
    s.src = src;
  }
};
export const inLoop = (map: Record<string, string>, src: string): void => {
  for (const s in map) {
    (s as unknown as HTMLImageElement).src = src;
  }
};
export const classicLoop = (list: HTMLImageElement[], src: string): void => {
  for (let s = list[0]; s; s = list[1]) {
    s.src = src;
  }
};
export const loopScript = (src: string): void => {
  for (let s = document.createElement("script"); ; ) {
    s.src = src;
    break;
  }
};
export const top = (src: string): void => {
  s.src = src;
};
