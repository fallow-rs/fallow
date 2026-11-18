import { direct } from "./orders/direct";
import { forbidden } from "./orders/forbidden";
import { viaInternalBarrel } from "./orders/via-internal-barrel";
import { viaLeak } from "./orders/via-leak";
import { viaPublic } from "./orders/via-public";
import { viaShared } from "./orders/via-shared";
import { viaStar } from "./orders/via-star";
import { shipTypeDeep } from "./shipping/type-deep";
import { shipTypeViaShared } from "./shipping/type-via-shared";
import { shipViaShared } from "./shipping/via-shared";

console.log(
  direct(),
  forbidden(),
  viaInternalBarrel(),
  viaLeak(),
  viaPublic(),
  viaShared(),
  viaStar(),
  shipViaShared(),
  shipTypeViaShared({ id: 1 }),
  shipTypeDeep({ id: 2 }),
);
