import type { RootStore } from "./root";
import type { IIssueDetail } from "./store";

declare const context: RootStore;

export const useIssueDetail = (): IIssueDetail => context.issueDetail;
