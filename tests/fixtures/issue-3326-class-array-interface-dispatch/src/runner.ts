import type { Controller as ParameterController } from './parameter';
import type { Controller as FieldController } from './field';
import type { Controller as FunctionController } from './function';

export class ParameterRunner {
  run() {
    this.layers.forEach(layer => layer.init?.());
    for (const layer of this.layers) layer.tick?.();
  }
  constructor(private layers: ParameterController[]) {}
}

export class FieldRunner {
  run() {
    this.layers.forEach(layer => layer.init?.());
    for (const layer of this.layers) layer.tick?.();
  }
  private layers: FieldController[];
  constructor(layers: FieldController[]) { this.layers = layers; }
}

export const runFunction = (layers: FunctionController[]): void => {
  layers.forEach(layer => layer.init?.());
  for (const layer of layers) layer.tick?.();
};
