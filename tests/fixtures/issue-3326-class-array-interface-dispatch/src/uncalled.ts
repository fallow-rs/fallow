export interface Controller { init?: () => void; tick?: () => void; unused?: () => void; }
export class UncalledAdapter implements Controller { init() {} tick() {} unused() {} }
