// Generated from tools/mutation-runner/src/runtime/replay-value.mjs. Do not edit.
/**
 * The JSON encoding of replay arguments and results (replay contract 1).
 *
 * A JSON value that has no exact JSON form becomes an object with the tag
 * key `$replay`: `undefined`, a number that is not finite or is `-0`, a
 * bigint, a `Date`, a `RegExp`, a `Map`, a `Set` and a typed array. A plain
 * object with an own `$replay` key becomes `{ $replay: "object", entries }`,
 * so the tag key of user data never reads as a tag.
 *
 * An argument must decode to an equal value in a new process. A function, a
 * symbol, a class instance, a proxy, a cyclic value and an accessor property
 * do not, so `encodeArgs` refuses them with the reason. A stream and a DOM
 * node are class instances. A result needs only a stable text for the
 * comparison, so `encodeResult` encodes every value.
 */

const TAG = "$replay";
const MAX_DEPTH = 32;
const MAX_NODES = 10_000;

const TYPED_ARRAYS = {
  Int8Array,
  Uint8Array,
  Uint8ClampedArray,
  Int16Array,
  Uint16Array,
  Int32Array,
  Uint32Array,
  Float32Array,
  Float64Array,
  BigInt64Array,
  BigUint64Array,
};

/** An error that ends the encoding of an argument with its reason. */
class NotReplayable extends Error {}

const isProxy = (() => {
  const types = globalThis.process?.getBuiltinModule?.("node:util")?.types;
  return typeof types?.isProxy === "function" ? (value) => types.isProxy(value) : () => false;
})();

const specialNumber = (value) => {
  if (Number.isNaN(value)) return "NaN";
  if (value === Number.POSITIVE_INFINITY) return "Infinity";
  if (value === Number.NEGATIVE_INFINITY) return "-Infinity";
  if (Object.is(value, -0)) return "-0";
  return null;
};

const typedArrayName = (value) =>
  Object.keys(TYPED_ARRAYS).find((name) => value instanceof TYPED_ARRAYS[name]) ?? null;

const isPlainPrototype = (value) => {
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
};

const className = (value) => {
  try {
    return value.constructor?.name ?? "";
  } catch {
    return "";
  }
};

/** The encoders of the values that `typeof` names, and that JSON can hold. */
const PRIMITIVES = {
  undefined: () => ({ [TAG]: "undefined" }),
  boolean: (value) => value,
  string: (value) => value,
  number: (value) => {
    const special = specialNumber(value);
    return special === null ? value : { [TAG]: "number", value: special };
  },
  bigint: (value) => ({ [TAG]: "bigint", value: String(value) }),
};

/** The values that cannot be arguments: the reason and the result tag. */
const OPAQUE = {
  symbol: {
    reason: "a symbol",
    tag: (value) => ({ [TAG]: "symbol", description: value.description ?? null }),
  },
  function: { reason: "a function", tag: (value) => ({ [TAG]: "function", name: value.name }) },
};
const PROXY = { reason: "a proxy", tag: () => ({ [TAG]: "proxy" }) };
const CYCLE = { reason: "a cyclic value", tag: () => ({ [TAG]: "cycle" }) };
const TOO_LARGE = {
  reason: "a value that is too large or too deep",
  tag: () => ({ [TAG]: "truncated" }),
};

/**
 * Encodes one value. `strict` refuses a value that cannot be an argument.
 * `path` holds the objects on the way down, so a cycle is found.
 */
const createEncoder = (strict) => {
  let nodes = 0;
  const opaque = (kind, value) => {
    if (strict) throw new NotReplayable(kind.reason);
    return kind.tag(value);
  };

  const encodeFields = (value, path, depth) => {
    const fields = {};
    for (const key of Reflect.ownKeys(value)) {
      if (typeof key === "symbol") {
        if (strict) throw new NotReplayable("an object with a symbol key");
        continue;
      }
      const descriptor = Reflect.getOwnPropertyDescriptor(value, key);
      if (!descriptor.enumerable) continue;
      fields[key] =
        descriptor.get === undefined && descriptor.set === undefined
          ? encode(descriptor.value, path, depth + 1)
          : opaque({
              reason: `an accessor property \`${key}\``,
              tag: () => ({ [TAG]: "accessor" }),
            });
    }
    return fields;
  };

  const encodeArray = (value, path, depth) =>
    Array.from({ length: value.length }, (_, index) =>
      Object.hasOwn(value, index) ? encode(value[index], path, depth + 1) : { [TAG]: "undefined" },
    );

  /** The built-in classes with an exact encoding, in check order. */
  const BUILT_INS = [
    [Array.isArray, encodeArray],
    [
      (value) => value instanceof Date,
      (value) => {
        const time = value.getTime();
        return { [TAG]: "date", value: Number.isNaN(time) ? null : value.toISOString() };
      },
    ],
    [
      (value) => value instanceof RegExp,
      (value) => ({ [TAG]: "regexp", source: value.source, flags: value.flags }),
    ],
    [
      (value) => value instanceof Map,
      (value, path, depth) => ({
        [TAG]: "map",
        entries: [...value].map(([key, item]) => [
          encode(key, path, depth + 1),
          encode(item, path, depth + 1),
        ]),
      }),
    ],
    [
      (value) => value instanceof Set,
      (value, path, depth) => ({
        [TAG]: "set",
        values: [...value].map((item) => encode(item, path, depth + 1)),
      }),
    ],
    [
      (value) => typedArrayName(value) !== null,
      (value) => ({
        [TAG]: "bytes",
        type: typedArrayName(value),
        values: [...value].map((item) => (typeof item === "bigint" ? String(item) : item)),
      }),
    ],
  ];

  const encodeObject = (value, path, depth) => {
    const builtIn = BUILT_INS.find(([matches]) => matches(value));
    if (builtIn !== undefined) return builtIn[1](value, path, depth);
    if (value instanceof Error && !strict) {
      return { [TAG]: "error", name: String(value.name), message: String(value.message) };
    }
    if (isPlainPrototype(value)) {
      const fields = encodeFields(value, path, depth);
      return Object.hasOwn(fields, TAG)
        ? { [TAG]: "object", entries: Object.entries(fields) }
        : fields;
    }
    if (strict) throw new NotReplayable(`a class instance (${className(value) || "anonymous"})`);
    return { [TAG]: "instance", class: className(value), fields: encodeFields(value, path, depth) };
  };

  const encodeReference = (value, path, depth) => {
    if (isProxy(value)) return opaque(PROXY, value);
    if (path.includes(value)) return opaque(CYCLE, value);
    path.push(value);
    try {
      return encodeObject(value, path, depth);
    } finally {
      path.pop();
    }
  };

  const encode = (value, path, depth) => {
    nodes += 1;
    if (nodes > MAX_NODES || depth > MAX_DEPTH) return opaque(TOO_LARGE, value);
    const kind = typeof value;
    if (Object.hasOwn(PRIMITIVES, kind)) return PRIMITIVES[kind](value);
    if (Object.hasOwn(OPAQUE, kind)) return opaque(OPAQUE[kind], value);
    return value === null ? null : encodeReference(value, path, depth);
  };

  return (value) => encode(value, [], 0);
};

/**
 * Encodes the arguments of one call. Gives `{ ok: true, value }`, or
 * `{ ok: false, reason }` for a value that cannot be replayed.
 */
export const encodeArgs = (args) => {
  try {
    return { ok: true, value: [...args].map(createEncoder(true)) };
  } catch (error) {
    if (error instanceof NotReplayable) return { ok: false, reason: error.message };
    return { ok: false, reason: `the value cannot be read: ${error?.message ?? error}` };
  }
};

/** Encodes a returned value or a thrown value. A read that throws gives a tag. */
export const encodeResult = (value) => {
  try {
    return createEncoder(false)(value);
  } catch {
    return { [TAG]: "unreadable" };
  }
};

/** The decoders of the tags that an argument can hold. */
const DECODERS = {
  undefined: () => undefined,
  number: (value) => Number(value.value === "-0" ? -0 : value.value),
  bigint: (value) => BigInt(value.value),
  date: (value) => new Date(value.value ?? Number.NaN),
  regexp: (value) => new RegExp(value.source, value.flags),
  map: (value) =>
    new Map(value.entries.map(([key, item]) => [decodeValue(key), decodeValue(item)])),
  set: (value) => new Set(value.values.map(decodeValue)),
  bytes: (value) => {
    const Type = TYPED_ARRAYS[value.type];
    if (Type === undefined) throw new TypeError(`unknown typed array ${value.type}`);
    return Type.from(value.values, (item) => (typeof item === "string" ? BigInt(item) : item));
  },
  object: (value) =>
    Object.fromEntries(value.entries.map(([key, item]) => [key, decodeValue(item)])),
};

const decodeTagged = (value) => {
  const tag = value[TAG];
  if (typeof tag !== "string" || !Object.hasOwn(DECODERS, tag)) {
    throw new TypeError(`the tag ${JSON.stringify(tag)} is not an argument value`);
  }
  return DECODERS[tag](value);
};

/** Decodes one encoded argument. Throws a `TypeError` for an invalid value. */
export const decodeValue = (value) => {
  if (value === null || typeof value !== "object") return value;
  if (Array.isArray(value)) return value.map(decodeValue);
  if (Object.hasOwn(value, TAG)) return decodeTagged(value);
  return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, decodeValue(item)]));
};
