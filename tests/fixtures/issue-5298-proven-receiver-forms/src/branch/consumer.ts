import { getPageService } from "./service";

export class TitleManager {
  update(kind: string): void {
    const service = getPageService(kind);
    service.branchUsed();
  }
}
