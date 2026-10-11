export interface Controller { init?: () => void; tick?: () => void; unused?: () => void; }
export class FieldAdapter implements Controller { init() {} tick() {} unused() {} }
