const WHOLE_NUMBER = /^\d+$/;
const FLAG = "flag";

const wholeNumber = (raw) => {
  const text = String(raw);
  const value = WHOLE_NUMBER.test(text) ? Number(text) : Number.NaN;
  return Number.isSafeInteger(value) ? value : null;
};

/**
 * Returns the whole number in `raw` when it is 1 or more. `raw` is a string
 * or a number. Throws `<name> must be a positive integer` for other input.
 */
export const parsePositiveInt = (name, raw) => {
  const value = wholeNumber(raw);
  if (value === null || value === 0) throw new Error(`${name} must be a positive integer`);
  return value;
};

/**
 * Returns the whole number in `raw` when it is 0 or more. Throws
 * `<name> must be a non-negative integer` for other input.
 */
export const parseNonNegativeInt = (name, raw) => {
  const value = wholeNumber(raw);
  if (value === null) throw new Error(`${name} must be a non-negative integer`);
  return value;
};

const CONVERTERS = {
  string: (_name, raw) => raw,
  positiveInt: parsePositiveInt,
  nonNegativeInt: parseNonNegativeInt,
};

const optionOf = (spec, name) => (Object.hasOwn(spec, name) ? spec[name] : null);

const inlineValue = (spec, arg) => {
  const separator = arg.indexOf("=");
  if (separator === -1) return null;
  const name = arg.slice(0, separator);
  const option = optionOf(spec, name);
  if (!option || option.type === FLAG) return null;
  return { name, option, raw: arg.slice(separator + 1) };
};

const store = (values, option, name, raw) => {
  const value = CONVERTERS[option.type](name, raw);
  values[option.key] = option.repeat ? [...(values[option.key] ?? []), value] : value;
};

/**
 * Parses `argv` against `spec` and returns a copy of `defaults` with the
 * parsed values. `spec` maps an option name to `{ key, type, repeat }`.
 * `type` is "flag", "string", "positiveInt" or "nonNegativeInt". A value
 * option accepts `--name value` and `--name=value`. An option with
 * `repeat: true` collects its values in an array. An argument that is not an
 * own key of `spec` throws `Unknown argument: <arg>`.
 */
export const parseOptions = (argv, spec, defaults = {}) => {
  const values = { ...defaults };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const inline = inlineValue(spec, arg);
    if (inline) {
      store(values, inline.option, inline.name, inline.raw);
      continue;
    }
    const option = optionOf(spec, arg);
    if (!option) throw new Error(`Unknown argument: ${arg}`);
    if (option.type === FLAG) {
      values[option.key] = true;
      continue;
    }
    const next = argv[index + 1];
    if (next === undefined || optionOf(spec, next)) throw new Error(`Missing value for ${arg}`);
    store(values, option, arg, next);
    index += 1;
  }
  return values;
};
