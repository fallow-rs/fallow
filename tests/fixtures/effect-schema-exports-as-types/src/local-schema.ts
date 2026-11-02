import { Effect } from "effect";

const Schema = { Struct: (fields: object): object => fields };

export const NotEffectSchema = Schema.Struct({});

export const usedValue = Effect.void;
