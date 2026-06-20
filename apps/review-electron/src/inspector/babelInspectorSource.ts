import type { PluginObj, PluginPass } from "@babel/core";
import type * as BabelTypes from "@babel/types";
import { isAbsolute, relative } from "node:path";

type Api = { types: typeof BabelTypes };

/** `root`: the review/project root, so stamped paths match Fallow's workspace-relative paths. */
type Options = { root?: string };

/**
 * Babel plugin: stamp `data-fallow-source="file:line:col"` on every JSX element
 * (dev only). The W5 picker reads this attribute to map a clicked DOM node back
 * to its source, robustly across React versions (React 19 dropped fiber
 * `_debugSource`). Same approach as locatorjs / click-to-component.
 *
 * The file is made relative to `options.root` so it lives in the SAME path-space
 * as `fallow review` output, which is what lets the inspector JOIN a selection to
 * grounded facts.
 */
export const babelInspectorSource = ({ types: t }: Api, options: Options = {}): PluginObj => ({
  name: "fallow-inspector-source",
  visitor: {
    JSXOpeningElement(path, state: PluginPass) {
      const loc = path.node.loc;
      if (!loc) return;
      const already = path.node.attributes.some(
        (attr) =>
          t.isJSXAttribute(attr) &&
          t.isJSXIdentifier(attr.name) &&
          attr.name.name === "data-fallow-source",
      );
      if (already) return;
      const raw = state.filename ?? "unknown";
      const file = options.root && isAbsolute(raw) ? relative(options.root, raw) : raw;
      const value = `${file}:${loc.start.line}:${loc.start.column}`;
      path.node.attributes.push(
        t.jsxAttribute(t.jsxIdentifier("data-fallow-source"), t.stringLiteral(value)),
      );
    },
  },
});
