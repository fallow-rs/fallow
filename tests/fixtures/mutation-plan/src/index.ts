import { add, clamp } from "./math";
import { save, size } from "./writer";
import { untested } from "./untested";
import { renderWrapper } from "./wrapper";
import { schemaVersion } from "./schema.generated";
import { revalidate, render } from "./page";
import { __fallow_m_a as reserved } from "./reserved";

export function main(input: string): string {
  return `${add(1, 2)} ${clamp(5, 0, 3)} ${size(input)} ${untested(input)} ${renderWrapper()} ${schemaVersion()} ${revalidate} ${render()} ${reserved()}`;
}

export { save };
