export interface Controller { init?: () => void; tick?: () => void; unused?: () => void; }
export class ParameterAdapter implements Controller { init() {} tick() {} unused() {} }
