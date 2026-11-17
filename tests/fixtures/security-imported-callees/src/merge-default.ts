export default function (base: object, extra: object): object {
  const copy = { ...base };
  for (const [key, value] of Object.entries(extra)) {
    copy[key] = value;
  }
  return Object.freeze(copy);
}
