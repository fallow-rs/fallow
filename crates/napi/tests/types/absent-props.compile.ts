import {
  detectDeadCode,
  type AbsentComponentPropFinding,
  type DeadCodeOptions,
} from "../../types/index.js";

const options: DeadCodeOptions = { root: ".", absentComponentProps: true };
const consume = async (): Promise<void> => {
  const report = await detectDeadCode(options);
  const candidates: AbsentComponentPropFinding[] = report.absent_component_props ?? [];
  for (const candidate of candidates) {
    const defaults: boolean = candidate.has_default;
    const callers: string[] = candidate.inspected_call_sites.map((caller) => caller.path);
    const count: number = report.summary.absent_component_props;
    void defaults;
    void callers;
    void count;
  }
};
void consume;
