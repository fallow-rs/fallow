import { PageCoreService } from "./core";

export class PageService extends PageCoreService {
  protected basePath = "/pages";
}

export const getPageService = (kind: string) => {
  if (kind === "project") {
    return new PageService();
  }
  throw new Error(`Invalid kind ${kind}`);
};
