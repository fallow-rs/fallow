import {
  detectDeadCode,
  type AbsentComponentPropFinding,
  type ConstantParameterFinding,
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

const constantOptions: DeadCodeOptions = { root: ".", constantParameters: true };
const consumeConstantParameters = async (): Promise<void> => {
  const report = await detectDeadCode(constantOptions);
  const candidates: ConstantParameterFinding[] = report.constant_parameters ?? [];
  const total: number | undefined = report.summary.constant_parameters;
  void total;
  for (const candidate of candidates) {
    const reason: string = candidate.reason;
    const text: string | undefined = candidate.value?.text;
    const callers: string[] = candidate.inspected_call_sites.map((caller) => caller.path);
    void reason;
    void text;
    void callers;
  }
};
void consumeConstantParameters;
