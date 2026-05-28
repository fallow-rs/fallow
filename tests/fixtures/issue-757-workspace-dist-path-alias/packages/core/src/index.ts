import { greet } from "@fix757/utils";
import { slug } from "@fix757/utils/string";

export const run = (): string => greet() + slug("X");
