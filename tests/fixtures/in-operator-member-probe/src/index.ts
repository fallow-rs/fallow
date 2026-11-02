import { hasRetryHint, timeout } from "./probe";
import { RequestError, describe } from "./request";
import { TypedReport, toResult } from "./typed";
import { UnrelatedReport } from "./unrelated";

const error = new RequestError();
console.log(describe(error), hasRetryHint(error), timeout(), new UnrelatedReport());
console.log(new TypedReport(), toResult("value"));
