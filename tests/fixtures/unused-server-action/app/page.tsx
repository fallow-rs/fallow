import { CustomForm } from "./components/CustomForm";
import { formAction, callAction, propAction } from "./actions";
import { usedUtil } from "./lib/util";
import { usedInlineAction } from "./inline-actions";

// Entry point (App Router page). Consumes three of the file-level server actions:
//   - formAction via a native <form action={...}> binding
//   - callAction via a plain import-and-call
//   - propAction via a component `action` prop
// plus usedInlineAction (an inline "use server" body action). `deadAction`,
// `suppressedDeadAction`, `deadInlineAction`, and `deadInlineArrow` are
// referenced nowhere.
export default function Page() {
  callAction();
  usedUtil();
  usedInlineAction();
  return (
    <div>
      <form action={formAction}>
        <button type="submit">Go</button>
      </form>
      <CustomForm action={propAction} />
    </div>
  );
}
