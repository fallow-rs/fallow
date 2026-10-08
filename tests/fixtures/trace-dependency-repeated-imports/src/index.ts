import { createStore, combine } from "state-kit";
import { readMixed } from "./mixed";
import type { Shape } from "./types-only";

export const store: Shape = createStore(combine(readMixed()));
