// An import precedes the directive, so oxc parses `"use client"` as an ordinary
// string-literal expression statement in `program.body`, NOT a leading prologue
// directive. Next.js silently ignores it and treats this file as a server
// module. fallow flags it as a misplaced directive.
import { helper } from "./helper";

"use client";

export default function Page() {
  return <div>{helper}</div>;
}
