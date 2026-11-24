import { Schema } from "effect";
import * as FormHelpers from "./form-helpers";

const flag = Math.random() > 0.5;

export const FormTernary = flag ? Schema.String : Schema.Number;
export const formTernaryPlain = flag ? Schema.String : 1;

const pick = (strict: boolean) => (strict ? Schema.String : Schema.Number);
const pickPlain = (strict: boolean) => (strict ? Schema.String : 1);
export const FormConditionalHelper = pick(true);
export const formConditionalHelperPlain = pickPlain(true);

export const FormNsHelper = FormHelpers.makeText();
export const FormNsMember = FormHelpers.Text.pipe(Schema.brand("FormNsMember"));
export const formNsPlain = FormHelpers.makePlainValue();
export const formNsMemberPlain = FormHelpers.plainText;

const identity = <A>(value: A): A => value;
function firstOf<A, B>(first: A, second: B): A {
  console.log(second);
  return first;
}
function reassigned<A>(value: A): A {
  value = 1 as A;
  return value;
}
export const FormParam = identity(Schema.String);
export const FormParamBlock = firstOf(Schema.String, 1);
export const formParamPlain = identity(1);
export const formParamOther = firstOf(1, Schema.String);
export const formParamReassigned = reassigned(Schema.String);

const record = { make: () => Schema.String, plain: () => 1, User: Schema.String };
export const FormMethod = record.make();
export const FormRecordMember = record.User;
export const formMethodPlain = record.plain();

const tuple = [Schema.String, Schema.Number] as const;
const mixed = [Schema.String, 1] as const;
const written = [Schema.String];
written[0] = 1 as never;
const escaped = [Schema.String];
console.log(escaped);
const withThis = {
  value: Schema.String,
  reset() {
    this.value = Schema.Number;
  },
};
export const FormIndex = tuple[0];
export const FormIndexString = tuple["1"];
export const formIndexPlain = mixed[1];
const short = [Schema.String];
export const formIndexOutOfRange = short[1];
export const formIndexWritten = written[0];
export const formIndexEscaped = escaped[0];
export const formThis = withThis.value;

export const { FormDestructured } = { FormDestructured: Schema.String };
export const [FormDestructuredTuple] = tuple;
export const { formDestructuredPlain } = { formDestructuredPlain: 1 };

const selfCall = { inner: (): unknown => selfCall.inner() };
export const formSelfCall = selfCall.inner();

const shifted = [Schema.String, Schema.Number];
shifted.shift();
const deleted = { User: Schema.String };
delete (deleted.User as never);
const assigned = { User: Schema.String };
(assigned.User as unknown) = 1;
const chained = { User: Schema.String };
delete chained?.User;
export const exportedSchemas = { User: Schema.String };
export const formShifted = shifted[0];
export const formDeleted = deleted.User;
export const formAssigned = assigned.User;
export const formChainDeleted = chained.User;
export const formExported = exportedSchemas.User;
const padded = [Schema.String];
export const formPaddedIndex = padded["00"];
