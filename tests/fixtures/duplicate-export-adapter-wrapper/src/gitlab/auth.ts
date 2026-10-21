import { configureAuth as configureAuthCore } from "../ci/auth";

export function configureAuth(env: { url: string }): string {
  return configureAuthCore(env.url);
}
