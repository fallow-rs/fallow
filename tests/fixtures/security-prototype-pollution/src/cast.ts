// A TypeScript cast on the assignment target still flattens to `obj.__proto__`,
// so the prototype write is a candidate.
export function castProto(obj: Record<string, unknown>, evil: unknown): void {
  (obj as { __proto__: unknown }).__proto__ = evil;
}
