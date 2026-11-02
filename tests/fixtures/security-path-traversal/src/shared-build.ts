// Build code that imports the same module as the handler.
import { sharedJoin } from "./shared-join";

export const buildShared = (name: string): string => sharedJoin(name);
