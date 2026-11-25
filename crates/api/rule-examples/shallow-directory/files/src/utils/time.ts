function pad(value: string): string {
  return value.padStart(2, '0');
}

export function formatDate(formatDateInput: string): string {
  return pad(formatDateInput);
}

export function parseDate(parseDateInput: string): string {
  return pad(parseDateInput);
}

export function addDays(addDaysInput: string): string {
  return pad(addDaysInput);
}

export function isWeekend(isWeekendInput: string): string {
  return pad(isWeekendInput);
}

