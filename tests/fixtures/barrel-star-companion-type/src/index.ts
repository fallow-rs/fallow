import { User } from "./barrel";
import type { User as UserType } from "./barrel";

export const parseUser = (input: unknown): UserType | null => {
  if (!User.decode(input)) {
    return null;
  }
  return input as UserType;
};
