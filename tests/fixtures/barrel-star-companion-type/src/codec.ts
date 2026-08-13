export const User = {
  decode: (input: unknown): boolean => typeof input === "object" && input !== null,
};
