import { encode as base64 } from "./Base64";
import { encode as url } from "./Url";

export const both = (value: string) => [base64(value), url(value)];
