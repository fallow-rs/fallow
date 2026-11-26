import { useIssueDetail } from "./hook";
import { RootStore } from "./root";

export const root = new RootStore();

export const useSubIssueOperations = (): void => {
  const { deleteSubIssue } = useIssueDetail();
  deleteSubIssue();
};
