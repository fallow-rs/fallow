import { AppContext } from "./context";
import { AppErrorHandler } from "./error-handler";
import { AuthMiddleware } from "./middleware";
import { PlainHandler } from "./plain-handler";

// The module receives these classes as providers. No code here calls their
// methods, so only the module dispatch can make them used.
const providers = [AppContext, AppErrorHandler, AuthMiddleware, PlainHandler];
for (const Provider of providers) {
  new Provider();
}
