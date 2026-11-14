import { provideAppInitializer } from "@angular/core";
import { isAllowed } from "./auth";

export const appConfig = {
  providers: [
    provideAppInitializer(() => {
      const ok = window.location.pathname === "/admin" && isAllowed(3);
      if (!ok) {
        throw new Error("denied");
      }
    }),
  ],
};
