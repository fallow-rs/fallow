export interface Entry {
  name: string;
}

export function usedReader(): Entry {
  return { name: 'used' };
}

export function unusedReader(): Entry {
  return { name: 'unused' };
}
