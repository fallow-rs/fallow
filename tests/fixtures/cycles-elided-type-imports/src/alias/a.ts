import { Shape, Units } from './b';
export const area = (shape: Shape, units: Units.Name): string => `${shape.width}${units}`;
