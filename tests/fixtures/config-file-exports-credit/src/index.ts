import { preset } from "../config/svgo.config.js";
import { language } from "./i18n";
import { handlerName } from "./lib/serverless";

export const app = `${language}:${handlerName}:${preset}`;
