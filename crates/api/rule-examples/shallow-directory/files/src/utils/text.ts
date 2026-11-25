function normalize(value: string): string {
  return value.trim();
}

export function slugify(slugifyInput: string): string {
  return normalize(slugifyInput);
}

export function truncate(truncateInput: string): string {
  return normalize(truncateInput);
}

export function capitalize(capitalizeInput: string): string {
  return normalize(capitalizeInput);
}

export function wordCount(wordCountInput: string): string {
  return normalize(wordCountInput);
}

