type Callback<T> = (name: string, body: (fixtures: T) => void) => void;

export const framework = {
  extend: <T>(_fixtures: object): Callback<T> => () => {},
};
