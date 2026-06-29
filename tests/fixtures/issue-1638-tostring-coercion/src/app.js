import { Money, Label, Tag, NotCoerced } from "./money.js";

// Template-literal interpolation coerces `Money`.
export const total = `Total: ${new Money(5)}`;

// `String(...)` coerces `Label`.
export const labelText = String(new Label());

// `+` with a string operand coerces `Tag`.
export const tagText = "tag:" + new Tag();

// Constructed but never coerced: `NotCoerced.toString` must still report.
export const leftover = new NotCoerced();
