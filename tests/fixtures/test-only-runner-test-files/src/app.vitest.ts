import leftPad from 'left-pad';
import { camelCase } from 'lodash';
import { app } from './index';

console.log(leftPad(app, 4), camelCase(app));
