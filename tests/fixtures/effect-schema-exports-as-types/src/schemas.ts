import { Schema } from "effect";

export class UnusedUser extends Schema.Class<UnusedUser>("UnusedUser")({
  id: Schema.String,
}) {
  get label(): string {
    return this.id;
  }
}

export const UnusedPayload = Schema.Struct({ name: Schema.String });

export const UnusedId = Schema.String.pipe(Schema.brand("UnusedId"));

export const UsedSchema = Schema.Struct({ id: Schema.String });

export const decodeUsed = Schema.decodeUnknownSync(UsedSchema);

export const unusedHelper = (): number => 1;

export type UnusedAlias = string;

// fallow-ignore-next-line unused-export
export const SuppressedSchema = Schema.Struct({});

export const PairedSchema = Schema.Struct({ id: Schema.String });
export type PairedSchema = typeof PairedSchema.Type;
