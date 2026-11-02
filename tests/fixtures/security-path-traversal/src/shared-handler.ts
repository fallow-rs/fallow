// A request handler that imports a module that build code also imports.
import { sharedJoin } from "./shared-join";

export const handleShared = (req: { url: string }): string => sharedJoin(req.url);
