import * as Cast from "./targets/cast";
import * as Assign from "./targets/assign";
import * as Call from "./targets/call";
import * as Spread from "./targets/spread";
import * as Computed from "./targets/computed";
import * as Keys from "./targets/keys";
import * as ByName from "./targets/byname";
import * as NoHide from "./targets/nohide";
import * as Barrel from "./barrel/index";

const record = Cast as Record<string, unknown>;
const alias = Assign;
const describe = (value: unknown): string => String(value);

export const formValues: unknown[] = [
  record["castUsed"],
  alias,
  describe(Call),
  { ...Spread },
  Computed["computed" + "Used"],
  Object.keys(Keys),
  ByName.bynameUsed,
  describe(NoHide),
  describe(Barrel),
];
