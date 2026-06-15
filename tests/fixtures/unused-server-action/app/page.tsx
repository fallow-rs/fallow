import { CustomForm } from "./components/CustomForm";
import { formAction, callAction, propAction } from "./actions";
import { usedUtil } from "./lib/util";

// Entry point (App Router page). Consumes three of the server actions:
//   - formAction via a native <form action={...}> binding
//   - callAction via a plain import-and-call
//   - propAction via a component `action` prop
// `deadAction` and `suppressedDeadAction` are referenced nowhere.
export default function Page() {
  callAction();
  usedUtil();
  return (
    <div>
      <form action={formAction}>
        <button type="submit">Go</button>
      </form>
      <CustomForm action={propAction} />
    </div>
  );
}
