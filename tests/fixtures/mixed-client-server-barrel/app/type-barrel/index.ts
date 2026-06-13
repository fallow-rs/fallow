// FALSE-POSITIVE NEGATIVE: the "use client" origin is re-exported TYPE-ONLY
// (`export type { ... }`), which is erased at build and carries no runtime
// directive context. Only the server-only origin contributes a runtime
// re-export, so there is no client+server mix and this MUST NOT flag.
export type { ButtonProps } from "./ClientTypes";
export { loadData } from "./serverData";
