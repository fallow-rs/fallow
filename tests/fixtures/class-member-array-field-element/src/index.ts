import { Month } from './month';

const month = new Month();
month.first();

export const anyFlag = (m: Month): boolean => m.days.some((day) => day.viaCallback());
export const reset = (m: Month): void => {
  for (const day of m.days) {
    day.viaLoop();
  }
};
export const firstOf = (m: Month): number => m.days[0].viaIndex();
export const sortedFirst = (m: Month): number => m.sorted[0].viaGetter();
