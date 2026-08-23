export namespace Outer {
  export import Inner = require('./inner');

  export const outerValue = Inner.innerValue;
}
