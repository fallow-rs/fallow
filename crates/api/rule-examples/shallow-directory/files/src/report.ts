import { slugify, truncate, capitalize, wordCount, formatDate, parseDate, addDays, isWeekend } from './utils';

export function report(input: string): string[] {
  return [slugify(input), truncate(input), capitalize(input), wordCount(input), formatDate(input), parseDate(input), addDays(input), isWeekend(input)];
}
