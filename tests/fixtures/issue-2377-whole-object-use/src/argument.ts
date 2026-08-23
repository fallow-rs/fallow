import * as ArgumentIcons from './argument-icons';

const register = (all: Record<string, unknown>): number => Object.keys(all).length;

export const useArgument = (): number => {
  ArgumentIcons.ArgumentStar();
  return register(ArgumentIcons);
};
