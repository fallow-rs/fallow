// An import precedes the directive, so oxc parses `"use client"` as an ordinary
// string-literal expression statement in `program.body`, NOT a leading prologue
// directive. Every React Server Components bundler (Waku included) silently
// ignores it and treats this file as a server module. fallow flags it as a
// misplaced directive: the rule is universal RSC semantics, not Next-specific.
import { helper } from "./helper";
import Widget from "./client-config";
import { usedAction } from "./server-action";

"use client";

export default function Page() {
  usedAction();
  return (
    <div>
      {helper}
      <Widget />
    </div>
  );
}
