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

/** The value of the first `<name>=<value>` argument in `argv`, else undefined. */
export const inlineOption = (argv, name) =>
  argv.find((arg) => arg.startsWith(`${name}=`))?.slice(name.length + 1);

const BENCH_COUNT_PARSERS = {
  runs: parsePositiveInt,
  warmup: parseNonNegativeInt,
  samples: parseNonNegativeInt,
};

const argName = (arg) => {
  const separator = arg.indexOf("=");
  return separator === -1 ? arg : arg.slice(0, separator);
};

/**
 * Checks the arguments of a benchmark script. `flags` are the names that
 * take no value; `values` are the names written as `--name=value`. Returns
 * null when each argument is known, else one line that names the first
 * problem.
 */
export const benchArgError = (argv, { flags = [], values = [] }) => {
  for (const arg of argv) {
    const name = argName(arg);
    const known = name === arg ? flags.includes(arg) : values.includes(name);
    if (known) continue;
    if (values.includes(arg)) return `Missing value for ${arg}: write ${arg}=VALUE`;
    return `Unknown argument: ${arg}`;
  }
  return null;
};

const exitWithError = (message) => {
  console.error(message);
  process.exit(1);
};

/**
 * Reads the repetition counts of a benchmark script from `--<key>=N`
 * arguments. `defaults` maps each key (`runs`, `warmup` or `samples`) to its
 * default. `runs` must be 1 or more; the other counts can be 0. `known`
 * lists the other `flags` and `values` of the script, as `benchArgError`
 * reads them. On an unknown argument or an invalid count the function prints
 * the error and exits the process with status 1, before the script starts a
 * build.
 */
export const readBenchCounts = (argv, defaults, known = {}) => {
  const countNames = Object.keys(defaults).map((key) => `--${key}`);
  const argError = benchArgError(argv, {
    flags: known.flags,
    values: [...(known.values ?? []), ...countNames],
  });
  if (argError) exitWithError(argError);
  try {
    return Object.fromEntries(
      Object.entries(defaults).map(([key, fallback]) => {
        const name = `--${key}`;
        return [key, BENCH_COUNT_PARSERS[key](name, inlineOption(argv, name) ?? fallback)];
      }),
    );
  } catch (error) {
    exitWithError(error.message);
  }
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

const projectSet = (raw) =>
  raw
    ? new Set(
        raw
          .split(",")
          .map((name) => name.trim())
          .filter(Boolean),
      )
    : null;

/**
 * Reads the shared arguments of the `bench*` scripts: `--synthetic` and
 * `--real-world` (both run when neither is given), `--projects=a,b` (null
 * when absent), and the counts of `readBenchCounts` for `counts`.
 */
export const readBenchArgs = (argv, counts) => {
  const counted = readBenchCounts(argv, counts, {
    flags: ["--synthetic", "--real-world"],
    values: ["--projects"],
  });
  const hasFilter = argv.includes("--synthetic") || argv.includes("--real-world");
  return {
    runSynthetic: argv.includes("--synthetic") || !hasFilter,
    runRealWorld: argv.includes("--real-world") || !hasFilter,
    projectFilter: projectSet(inlineOption(argv, "--projects")),
    ...counted,
  };
};
