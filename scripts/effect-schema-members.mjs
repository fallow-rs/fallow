#!/usr/bin/env node
/**
 * Classify each export of `effect/Schema` from its declared type, for the
 * member lists of `effect.schemaExportsAsTypes`.
 *
 * Usage:
 *   node scripts/effect-schema-members.mjs <project dir>
 *
 * The project folder must resolve `effect` and `typescript` (version 5, which
 * has the compiler API). The script prints a JSON snapshot with one entry per
 * value export:
 *   - `read`: the value itself is a schema (`Schema.String`);
 *   - `call`: a call gives a schema (`Schema.optional(...)`);
 *   - `transform`: a call gives a function from a schema to a schema, so
 *     `pipe` can take the call (`Schema.brand("Id")`);
 *   - `none`: no form gives a schema (decoders, guards, filters).
 *
 * Write the output to `crates/extract/tests/effect_schema_members/` as
 * `effect-<version>.json`. The Rust test `effect_schema_member_snapshots`
 * compares the lists of fallow with each snapshot there.
 */

import { createRequire } from "node:module";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const PROBE_FILE = "effect-schema-members-probe.ts";

/**
 * The type that every schema of the Effect major version is assignable to.
 * Effect 4 names the top type `Top`, and its generic members bound a schema
 * parameter with `Constraint`. In effect 3 every schema is assignable to
 * `Schema<any, any, any>`, because `any` satisfies each variance marker, and
 * `Schema.All` adds the schemas with a `never` type.
 */
export const topTypeFor = (major) =>
  major >= 4 ? "S.Top | S.Constraint" : "S.Schema<any, any, any> | S.Schema.All";

/**
 * The kinds of one member, from the predicates on its type:
 * `read`, `call`, `transform`, or `none` when no form gives a schema. A dual
 * member (both `call` and `transform`) also gets `arity:<n>`: a call needs at
 * least `n` arguments to give a schema, because a call with fewer arguments
 * is the data-last form and gives a function.
 */
export const memberKinds = ({ read, call, transform, callArity }) => {
  const kinds = [];
  if (read) kinds.push("read");
  if (call) kinds.push("call");
  if (transform) kinds.push("transform");
  if (call && transform) kinds.push(`arity:${callArity}`);
  return kinds.length > 0 ? kinds : ["none"];
};

/**
 * The snapshot text: one member per line, sorted by name, so a diff of two
 * snapshots shows each changed member and the Rust test can read the file
 * without a JSON parser.
 */
export const formatSnapshot = ({ effect, typescript, members }) => {
  const lines = Object.entries(members)
    .toSorted(([a], [b]) => a.localeCompare(b))
    .map(
      ([name, kinds]) =>
        `    ${JSON.stringify(name)}: ${JSON.stringify(kinds).replaceAll(",", ", ")}`,
    );
  return `{\n  "effect": ${JSON.stringify(effect)},\n  "typescript": ${JSON.stringify(typescript)},\n  "members": {\n${lines.join(",\n")}\n  }\n}\n`;
};

const createChecker = (ts, probePath) => {
  const program = ts.createProgram([probePath], {
    strict: true,
    noEmit: true,
    skipLibCheck: true,
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.NodeNext,
    moduleResolution: ts.ModuleResolutionKind.NodeNext,
  });
  const checker = program.getTypeChecker();
  const probeExports = checker.getExportsOfModule(
    checker.getSymbolAtLocation(program.getSourceFile(probePath)),
  );
  const exportNamed = (name) => probeExports.find((symbol) => symbol.name === name);
  return {
    checker,
    top: checker.getDeclaredTypeOfSymbol(exportNamed("ProbeTop")),
    namespace: checker.getAliasedSymbol(exportNamed("S")),
  };
};

const typePredicates = (ts, checker, top) => {
  const isSchema = (type) =>
    !(type.flags & (ts.TypeFlags.Any | ts.TypeFlags.Unknown | ts.TypeFlags.Never)) &&
    checker.isTypeAssignableTo(type, top);
  // A function from a schema to a schema: it returns a schema, and its first
  // parameter takes a schema or a type parameter bound by a schema
  // (`<S extends Top>(self: S)`).
  const isSchemaFunction = (type) =>
    type.getCallSignatures().some((sig) => {
      const [first] = sig.getParameters();
      if (!first || !isSchema(sig.getReturnType())) return false;
      const param = checker.getTypeOfSymbol(first);
      return isSchema(checker.getBaseConstraintOfType(param) ?? param);
    });
  const requiredArguments = (sig) =>
    sig
      .getParameters()
      .map((param) => param.valueDeclaration)
      .filter((decl) => decl && !decl.questionToken && !decl.initializer && !decl.dotDotDotToken)
      .length;
  // The fewest arguments of a call that gives a schema.
  const schemaCallArity = (type) =>
    Math.min(
      ...type
        .getCallSignatures()
        .filter((sig) => isSchema(sig.getReturnType()))
        .map(requiredArguments),
    );
  return { isSchema, isSchemaFunction, schemaCallArity };
};

const classify = (ts, probePath) => {
  const { checker, top, namespace } = createChecker(ts, probePath);
  const { isSchema, isSchemaFunction, schemaCallArity } = typePredicates(ts, checker, top);
  const members = {};
  for (const exported of checker.getExportsOfModule(namespace)) {
    const symbol =
      exported.flags & ts.SymbolFlags.Alias ? checker.getAliasedSymbol(exported) : exported;
    if (!(symbol.flags & ts.SymbolFlags.Value)) continue;
    const type = checker.getTypeOfSymbol(symbol);
    const results = type.getCallSignatures().map((sig) => sig.getReturnType());
    members[exported.name] = memberKinds({
      read: isSchema(type),
      call: results.some(isSchema),
      transform: results.some(isSchemaFunction),
      callArity: schemaCallArity(type),
    });
  }
  return members;
};

const main = (projectDir) => {
  if (!projectDir) {
    process.stderr.write("usage: effect-schema-members.mjs <project dir>\n");
    process.exit(2);
  }
  const root = resolve(projectDir);
  const resolveFromProject = createRequire(join(root, "package.json"));
  const ts = resolveFromProject("typescript");
  const effect = resolveFromProject(join("effect", "package.json")).version;
  const probeDir = mkdtempSync(join(root, ".effect-schema-members-"));
  const probePath = join(probeDir, PROBE_FILE);
  try {
    const topType = topTypeFor(Number(effect.split(".")[0]));
    writeFileSync(
      probePath,
      `import * as S from "effect/Schema";\nexport type ProbeTop = ${topType};\nexport { S };\n`,
    );
    const members = classify(ts, probePath);
    process.stdout.write(formatSnapshot({ effect, typescript: ts.version, members }));
  } finally {
    rmSync(probeDir, { recursive: true, force: true });
  }
};

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv[2]);
}
