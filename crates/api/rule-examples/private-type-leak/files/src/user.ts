interface InternalUser {
  id: number;
  name: string;
}

export function makeUser(name: string): InternalUser {
  return { id: Date.now(), name };
}
