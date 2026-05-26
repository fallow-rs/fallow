// Direct `extends Error`: `name` is runtime-used (logs, `err.name === ...`).
export class DomainError extends Error {
  name = "DomainError";
  // Non-`name` member with no usage: must still report as unused.
  unusedHelper(): void {}
}

// Transitive: extends a local subclass of Error.
export class ApiError extends DomainError {
  name = "ApiError";
}

// Extends a native error subclass directly (TypeError).
export class ValidationError extends TypeError {
  name = "ValidationError";
}

// Ordinary class: an unused `name` must still report.
export class Person {
  name = "anonymous";
}
