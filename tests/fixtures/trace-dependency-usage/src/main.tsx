import Counter from "./components/Counter";
import { Badge } from "./components/Badge";
import { App, hooks } from "./components/Namespace";
import { Root } from "./components/Alias";
import { useRootStore } from "./store/reexports";
import { load } from "./legacy";
import { useCount } from "./store/nested";
import { usePick, readCount, maybe } from "./store/local";
import type { Hook } from "./types-only";

export const all = [Counter, Badge, App, hooks, Root, useRootStore, load, useCount, usePick, readCount, maybe];
