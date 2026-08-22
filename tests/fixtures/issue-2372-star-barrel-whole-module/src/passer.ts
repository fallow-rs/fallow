import * as passed from './passed-barrel';

// A namespace binding handed on without any member access: the graph cannot
// narrow it, so it credits the whole namespace object.
export const passedOn = consume(passed);

function consume(value: unknown): unknown {
  return value;
}
