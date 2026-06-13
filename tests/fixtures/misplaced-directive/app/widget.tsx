"use client";

// Negative control: the directive is in the correct leading position (oxc puts
// it in `program.directives`), so nothing here is flagged.
import { helper } from "./helper";

export default function Widget() {
  return <div>{helper}</div>;
}
