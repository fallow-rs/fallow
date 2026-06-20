import { describe, it, expect } from "vitest";
import { transformSync } from "@babel/core";
import { babelInspectorSource } from "./babelInspectorSource";

const transform = (code: string): string =>
  transformSync(code, {
    filename: "/proj/src/Button.tsx",
    plugins: [[babelInspectorSource, { root: "/proj" }]],
    parserOpts: { plugins: ["jsx"] },
    configFile: false,
    babelrc: false,
  })?.code ?? "";

describe("babelInspectorSource", () => {
  it("stamps root-relative data-fallow-source on JSX elements", () => {
    const out = transform("const x = <div><span>hi</span></div>;");
    expect(out).toContain('data-fallow-source="src/Button.tsx:');
    expect((out.match(/data-fallow-source=/g) ?? []).length).toBe(2);
  });

  it("does not double-stamp", () => {
    const out = transform('const x = <div data-fallow-source="x:1:1">hi</div>;');
    expect((out.match(/data-fallow-source=/g) ?? []).length).toBe(1);
  });
});
