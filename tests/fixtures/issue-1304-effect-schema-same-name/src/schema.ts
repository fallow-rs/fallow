import { Schema } from "effect";

export const ServiceCategoryResponse = Schema.Struct({
  id: Schema.BigInt.annotations({
    description: "Unique identifier for the service category",
  }),
  slug: Schema.String.annotations({
    description: "URL-friendly identifier",
    examples: ["private-taxes", "fibu", "lohn"],
  }),
  displayName: Schema.String.annotations({
    description: "Human-readable name",
    examples: ["Private Taxes", "Financial Accounting"],
  }),
  shortName: Schema.String.pipe(Schema.NullishOr).annotations({
    description: "Optional abbreviated name",
  }),
  order: Schema.Number.pipe(Schema.NullishOr).annotations({
    description: "Display order (lower numbers first)",
  }),
}).annotations({
  identifier: "ServiceCategoryResponse",
  title: "Service Category",
  description: "A service category that can be associated with assistant prompts",
});
export type ServiceCategoryResponse = Schema.Schema.Type<
  typeof ServiceCategoryResponse
>;

export const AssistantPromptResponse = Schema.Struct({
  id: Schema.String.annotations({
    description: "Unique identifier for the assistant prompt",
  }),
  title: Schema.String.annotations({
    description: "The title of the assistant prompt",
  }),
  serviceCategories: Schema.Array(ServiceCategoryResponse).annotations({
    description: "Service categories associated with this prompt",
  }),
}).annotations({
  identifier: "AssistantPromptResponse",
  title: "Assistant Prompt",
});
export type AssistantPromptResponse = Schema.Schema.Type<
  typeof AssistantPromptResponse
>;

export const UnusedSiblingSchema = Schema.Struct({
  id: Schema.String,
});

export const OrphanChildSchema = Schema.Struct({
  id: Schema.String,
});

export const UnusedParentSchema = Schema.Struct({
  children: Schema.Array(OrphanChildSchema),
});
