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

export { RECORD_CALL_LIMIT, RECORD_LIMIT } from "./records.mjs";
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

// Without `util.types.isProxy` (browser mode, Node before 20.16), the encoder
// cannot tell a proxy, so an argument object is not replayable: reading it
// could run the traps of a proxy.
const proxyCheck = (() => {
  const types = globalThis.process?.getBuiltinModule?.("node:util")?.types;
  return typeof types?.isProxy === "function" ? (value) => types.isProxy(value) : null;
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

/** The prototypes of the built-in classes that an argument can hold. */
const BUILT_IN_PROTOTYPES = new Set(
  [Array, Date, RegExp, Map, Set, ...Object.values(TYPED_ARRAYS)].map((type) => type.prototype),
);

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
const TOO_LARGE = { reason: "a value that is too large", tag: () => ({ [TAG]: "truncated" }) };
const UNCHECKED = {
  reason: "an object that this runtime cannot check for a proxy",
  tag: () => ({ [TAG]: "unchecked" }),
};
const ACCESSOR = { reason: "an accessor property", tag: () => ({ [TAG]: "accessor" }) };

/**
 * The number of items that a built-in collection holds, or 0. The size comes
 * from the getter of the built-in prototype, so a subclass with its own
 * `length` or `size` getter runs no code here.
 */
const TYPED_LENGTH = Object.getOwnPropertyDescriptor(
  Object.getPrototypeOf(Uint8Array.prototype),
  "length",
).get;
const MAP_SIZE = Object.getOwnPropertyDescriptor(Map.prototype, "size").get;
const SET_SIZE = Object.getOwnPropertyDescriptor(Set.prototype, "size").get;
const itemCount = (value) => {
  if (Array.isArray(value)) return Reflect.getOwnPropertyDescriptor(value, "length").value;
  if (value instanceof Map) return MAP_SIZE.call(value);
  if (value instanceof Set) return SET_SIZE.call(value);
  return typedArrayName(value) === null ? 0 : TYPED_LENGTH.call(value);
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
      const field =
        descriptor.get === undefined && descriptor.set === undefined
          ? encode(descriptor.value, path, depth + 1)
          : opaque(ACCESSOR);
      // A plain assignment of the key `__proto__` sets the prototype.
      Object.defineProperty(fields, key, {
        value: field,
        enumerable: true,
        writable: true,
        configurable: true,
      });
    }
    return fields;
  };

  // Each index goes through its descriptor, so a getter never runs.
  const encodeArray = (value, path, depth) =>
    Array.from({ length: value.length }, (_, index) => {
      const descriptor = Reflect.getOwnPropertyDescriptor(value, index);
      if (descriptor === undefined) return { [TAG]: "undefined" };
      if (descriptor.get !== undefined || descriptor.set !== undefined) return opaque(ACCESSOR);
      return encode(descriptor.value, path, depth + 1);
    });

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
        values: [...value].map((item) =>
          typeof item === "bigint" ? String(item) : PRIMITIVES.number(item),
        ),
      }),
    ],
  ];

  /**
   * The encoder of a built-in class. A subclass (for example `Buffer`)
   * decodes as its base class, without its methods, so it is not the same
   * argument.
   */
  const builtInEncoder = (value) => {
    const builtIn = BUILT_INS.find(([matches]) => matches(value));
    if (builtIn === undefined) return undefined;
    if (strict && !BUILT_IN_PROTOTYPES.has(Object.getPrototypeOf(value))) {
      throw new NotReplayable(`a class instance (${className(value) || "anonymous"})`);
    }
    return builtIn[1];
  };

  const encodeObject = (value, path, depth) => {
    const builtIn = builtInEncoder(value);
    if (builtIn !== undefined) return builtIn(value, path, depth);
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
    if (proxyCheck === null) {
      if (strict) return opaque(UNCHECKED, value);
    } else if (proxyCheck(value)) {
      return opaque(PROXY, value);
    }
    if (path.includes(value)) return opaque(CYCLE, value);
    // The size check comes before any copy of a large collection.
    if (nodes + itemCount(value) > MAX_NODES) return opaque(TOO_LARGE, value);
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
    return { ok: false, reason: "a value that cannot be read" };
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
    return Type.from(value.values, (item) =>
      typeof item === "string" ? BigInt(item) : decodeValue(item),
    );
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
