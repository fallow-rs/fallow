import { IssueDetail, type IIssueDetail } from "./store";

export class RootStore {
  issueDetail: IIssueDetail;

  constructor() {
    this.issueDetail = new IssueDetail(this);
  }
}
