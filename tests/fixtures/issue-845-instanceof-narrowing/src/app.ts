import { BaseException } from "./exceptions";

// `getMessage` is only ever reached through the instanceof-narrowed local `e`.
export function handle(e: unknown): string | undefined {
  if (e instanceof BaseException) {
    return e.getMessage();
  }
  return undefined;
}
