import { bTest } from "../src/b-fixture";
import { cTest } from "../src/c-fixture";
import { dTest } from "../src/d-fixture";
import { eTest } from "../src/e-fixture";
import { makeHelperTest } from "../src/helper-fixture";

bTest("opens b", async ({ app, b }) => {
  app.openApp();
  b.openB();
});

cTest("opens c", async ({ c }) => {
  c.openC();
});

dTest("opens d", async ({ d }) => {
  d.openD();
});

eTest("opens e", async ({ e }) => {
  e.openE();
});

makeHelperTest()("opens helper", async ({ helper }) => {
  helper.openHelper();
});
