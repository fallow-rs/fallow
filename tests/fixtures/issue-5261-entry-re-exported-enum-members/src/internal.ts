import { Alias } from './alias';
import { JobName } from './client';
import { Mode } from './inner/deep';
import { Kind } from './kind';
import { Level } from './levels';
import { Priority, Unpublished } from './named';

export enum Internal {
  Used = 'used',
  Unused = 'unused',
}

export const internal = (): string[] => [
  Alias.Used,
  JobName.Run,
  Kind.Used,
  Mode.Fast,
  Level.Debug,
  Priority.Low,
  Unpublished.Used,
];
