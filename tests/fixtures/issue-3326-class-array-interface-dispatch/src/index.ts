import { ParameterAdapter } from './parameter';
import { FieldAdapter } from './field';
import { FunctionAdapter } from './function';
import { UncalledAdapter } from './uncalled';
import { ParameterRunner, FieldRunner, runFunction } from './runner';

new ParameterRunner([new ParameterAdapter()]).run();
new FieldRunner([new FieldAdapter()]).run();
runFunction([new FunctionAdapter()]);
new UncalledAdapter();
