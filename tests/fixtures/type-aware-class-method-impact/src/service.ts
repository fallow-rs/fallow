import { UserRepository } from "./repository.js";

export const persistUser = (): void => {
  new UserRepository().save();
};
