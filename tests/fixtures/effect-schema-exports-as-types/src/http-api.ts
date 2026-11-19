import { Schema } from "effect";
import * as HttpApiSchema from "effect/http-api/HttpApiSchema";
import { HttpApiSchema as PlatformHttpApiSchema } from "@effect/platform";

export const FormRequest = Schema.Struct({ scope: Schema.String }).pipe(
  HttpApiSchema.asFormUrlEncoded(),
);

export const CreatedBody = Schema.String.pipe(
  Schema.minLength(1),
  PlatformHttpApiSchema.withEncoding({ kind: "Text" }),
);

export const httpStatusProbe = Schema.String.pipe(HttpApiSchema.isNoContent);

export const bareHttpTransform = Schema.String.pipe(HttpApiSchema.asJson);
