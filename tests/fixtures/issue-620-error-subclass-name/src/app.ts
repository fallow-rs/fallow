import { DomainError, ApiError, ValidationError, Person } from "./errors";

export function run(): void {
  throw new DomainError();
}

export function runApi(): void {
  throw new ApiError();
}

export function validate(): void {
  throw new ValidationError();
}

export function makePerson(): Person {
  return new Person();
}
