import * as more from "./more";
import { Api, Split, Dotted } from "./more";

export type R = Api.Types.Request;
const escaped = Api.Escaped;
export const e = escaped;
export const viaImport = more.Api.viaNamespaceImport;
export type T = typeof Api.viaTypeof;
export const usesHidden = Api.usesHidden;
export const splitSecond = Split.second;
export const dottedUsed = Dotted.Inner.used;
