import type { ILinkActions, ISubIssueActions } from "./sub-actions";

interface ILocalActions {
  localAction: () => void;
}

interface IGrandparentActions {
  grandparentAction(): void;
}

interface IParentActions extends IGrandparentActions {
  parentAction(): void;
}

export interface IIssueDetail extends ISubIssueActions, ILinkActions, ILocalActions, IParentActions {
  fetchIssue: () => void;
}

export class IssueDetail implements IIssueDetail {
  constructor(private readonly root: unknown) {}

  deleteSubIssue = (): void => {};

  optionalSubIssue = (): void => {};

  fetchLinks = (): void => {};

  localAction = (): void => {};

  parentAction(): void {}

  grandparentAction(): void {}

  fetchIssue = (): void => {};

  storeOnlyMethod(): void {}
}
