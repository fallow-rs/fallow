import { createElement } from "react";
import { helper } from "@scope/pkg";
import type { Feature } from "geojson";

export const view = createElement("div", null, helper());
export const emptyFeature = (): Feature | null => null;
