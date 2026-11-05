import { mode } from "./mode";

(globalThis as { mode?: string }).mode = mode;
