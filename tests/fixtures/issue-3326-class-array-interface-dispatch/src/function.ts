export interface Controller { init?: () => void; tick?: () => void; unused?: () => void; }
export class FunctionAdapter implements Controller { init() {} tick() {} unused() {} }
