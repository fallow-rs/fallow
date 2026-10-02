export interface Settings {
  enabled: boolean;
}

export function load(): Settings {
  return { enabled: true };
}
