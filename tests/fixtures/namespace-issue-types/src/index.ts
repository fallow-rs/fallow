import { members } from "./barrel";
import * as escaped from "./escaped";
import { Legacy } from "./legacy";

declare function register(api: unknown): void;

// The consumer reads one value and one type through the namespace.
export const value = members.usedValue;
export type Value = members.UsedType;

// The namespace object escapes, so the consumer can read every export.
register(escaped);

export const inner = Legacy.usedInner;
