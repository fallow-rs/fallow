// A tiny TypeScript project that holds one example of each problem that the
// prompts ask about, so an agent that reads the files sees a real target.

import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

/** The temp prefix starts with `fallow-`, so `check-temp-leaks.mjs` sees a leak. */
const TEMP_PREFIX = "fallow-agent-pick-";

const FILES = {
  "package.json": `${JSON.stringify(
    {
      name: "shop-app",
      version: "1.0.0",
      private: true,
      type: "module",
      scripts: { build: "tsc -p .", test: "vitest run" },
      dependencies: { "date-fns": "^4.1.0", lodash: "^4.17.21", zod: "^3.24.0" },
      devDependencies: { typescript: "^5.7.0", vitest: "^3.0.0" },
    },
    null,
    2,
  )}\n`,
  "tsconfig.json": `${JSON.stringify(
    {
      compilerOptions: {
        target: "ES2022",
        module: "NodeNext",
        moduleResolution: "NodeNext",
        strict: true,
        outDir: "dist",
      },
      include: ["src"],
    },
    null,
    2,
  )}\n`,
  "src/index.ts": `import { renderCart } from "./ui/cart.js";
import { loadCart } from "./db/cart-store.js";

export const main = async (): Promise<void> => {
  const cart = await loadCart("user-1");
  console.log(renderCart(cart));
};
`,
  "src/ui/cart.ts": `import { z } from "zod";
import type { Cart } from "../db/cart-store.js";
import { cartTotal } from "../pricing.js";

export const CartSchema = z.object({ id: z.string() });

export const renderCart = (cart: Cart): string => \`\${cart.items.length} items, \${cartTotal(cart)}\`;

export const formatPrice = (cents: number): string => \`EUR \${(cents / 100).toFixed(2)}\`;
`,
  "src/db/cart-store.ts": `import { applyDiscount } from "../pricing.js";

export interface Cart {
  userId: string;
  items: Array<{ sku: string; cents: number; quantity: number }>;
}

export const loadCart = async (userId: string): Promise<Cart> => ({
  userId,
  items: [{ sku: "a", cents: applyDiscount(1000, "none"), quantity: 1 }],
});
`,
  "src/pricing.ts": `import type { Cart } from "./db/cart-store.js";

export const cartTotal = (cart: Cart): number => {
  let total = 0;
  for (const item of cart.items) {
    total += item.cents * item.quantity;
  }
  return total;
};

export const orderTotal = (cart: Cart): number => {
  let total = 0;
  for (const item of cart.items) {
    total += item.cents * item.quantity;
  }
  return total;
};

export const applyDiscount = (cents: number, code: string): number => {
  if (code === "none") return cents;
  if (code.startsWith("VIP")) {
    if (cents > 10000) return cents * 0.8;
    else if (cents > 5000) return cents * 0.85;
    else return cents * 0.9;
  } else if (code.startsWith("SALE")) {
    for (let i = 0; i < 3; i++) {
      if (cents > 1000 * (i + 1)) cents -= 100;
    }
    return cents;
  } else if (code === "FREE" && cents < 2000) {
    return 0;
  }
  return cents;
};
`,
  "src/legacy/old-checkout.ts": `export const legacyCheckout = (): string => "unused";
`,
};

/**
 * Create a run directory with the fixture project in `fixture/`.
 * @returns {{ runDir: string, fixtureDir: string, remove: () => void }}
 */
export const createFixture = () => {
  const runDir = mkdtempSync(join(tmpdir(), TEMP_PREFIX));
  const fixtureDir = join(runDir, "fixture");
  for (const [path, content] of Object.entries(FILES)) {
    const target = join(fixtureDir, path);
    mkdirSync(dirname(target), { recursive: true });
    writeFileSync(target, content);
  }
  return {
    runDir,
    fixtureDir,
    remove: () => rmSync(runDir, { recursive: true, force: true }),
  };
};
