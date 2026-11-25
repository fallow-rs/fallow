import path from "node:path";

import { ast, projectCompilerOptions, sync } from "./backend.mjs";
import { projectPackage } from "./project-package.mjs";
import {
  declarationName,
  declarationsForSymbol,
  isProjectSource,
  projectSourceFiles,
  resolveAlias,
  sourceFileIdentity,
  stableDeclarationKey,
} from "./semantic-identity.mjs";
import { recordBoundedUse } from "./symbol-use-evidence.mjs";

const { NodeBuilderFlags, SignatureKind, SymbolFlags } = sync;
const {
  isClassDeclaration,
  isEnumDeclaration,
  isExportAssignment,
  isExportDeclaration,
  isFunctionDeclaration,
  isGetAccessorDeclaration,
  isImportDeclaration,
  isInterfaceDeclaration,
  isMethodDeclaration,
  isPrivateIdentifier,
  isPropertyDeclaration,
  isVariableDeclaration,
} = ast;

/**
 * Declaration emit writes the name of an interface, a class or an enum. It
 * cannot inline these types, so the name must be importable from the module
 * that declares it. A type alias is different: when the alias has no export,
 * declaration emit writes its structure, so an inferred type never needs the
 * export of an alias.
 */
const NAMED_TYPE_FLAGS = SymbolFlags.Interface | SymbolFlags.Class | SymbolFlags.Enum;

// The type of a function or a namespace. Emit writes `typeof g` for one that
// the code can import, and the structure only for a local one.
const VALUE_TYPE_FLAGS = SymbolFlags.Function | SymbolFlags.ValueModule;

// The walk stops after this number of types in one file. A stopped walk
// credits less, so the finding stays.
const TYPE_BUDGET_PER_FILE = 10_000;

const safe = (operation, fallback) => {
  try {
    return operation() ?? fallback;
  } catch {
    return fallback;
  }
};

const hasDeclarationOutput = (packageJson) =>
  typeof packageJson.types === "string" ||
  typeof packageJson.typings === "string" ||
  JSON.stringify(packageJson.exports ?? null).includes('"types"');

/**
 * Only a project that emits declaration files needs the export. A bundler
 * plugin can write the declarations without `declaration` in the tsconfig,
 * so a `types` entry in `package.json` counts too.
 */
const emitsDeclarations = (root, project) => {
  const options = projectCompilerOptions(project);
  if (options.declaration || options.composite) return true;
  return hasDeclarationOutput(projectPackage(root, project).json);
};

const isNamedEmitTarget = (declaration) =>
  isInterfaceDeclaration(declaration) ||
  isClassDeclaration(declaration) ||
  isEnumDeclaration(declaration);

// Declaration emit writes a private member without its type.
const isPrivateMember = (member, sourceFile) =>
  Boolean(member.name && isPrivateIdentifier(member.name)) ||
  Boolean(member.modifiers?.some((modifier) => modifier.getText(sourceFile) === "private"));

const symbolType = (project, node) => {
  const symbol = project.checker.getSymbolAtLocation(node.name ?? node);
  if (!symbol) return undefined;
  return safe(() => project.checker.getTypeOfSymbolAtLocation(symbol, node), undefined);
};

const held = (holder, types) => types.filter(Boolean).map((type) => ({ type, holder }));

const returnTypes = (project, node) => {
  const type = symbolType(project, node);
  if (!type) return [];
  return safe(() => project.checker.getSignaturesOfType(type, SignatureKind.Call), []).map(
    (signature) => safe(() => project.checker.getReturnTypeOfSignature(signature), undefined),
  );
};

const inferredMemberTypes = (project, declaration) => {
  const sourceFile = declaration.getSourceFile();
  return declaration.members.flatMap((member) => {
    if (member.type || isPrivateMember(member, sourceFile)) return [];
    if (isPropertyDeclaration(member)) return held(member, [symbolType(project, member)]);
    if (isMethodDeclaration(member) || isGetAccessorDeclaration(member)) {
      return held(member, returnTypes(project, member));
    }
    return [];
  });
};

/**
 * The inferred types that declaration emit writes for one exported
 * declaration. A type annotation names its types in the source, so the
 * syntactic pass already credits them.
 */
const inferredDeclarationTypes = (project, declaration) => {
  if (isExportAssignment(declaration)) {
    return held(declaration, [
      safe(() => project.checker.getTypeAtLocation(declaration.expression), undefined),
    ]);
  }
  if (isFunctionDeclaration(declaration) && !declaration.type) {
    return held(declaration, returnTypes(project, declaration));
  }
  if (isClassDeclaration(declaration)) return inferredMemberTypes(project, declaration);
  if (isVariableDeclaration(declaration) && !declaration.type) {
    return held(declaration, [symbolType(project, declaration)]);
  }
  return [];
};

const moduleExports = (project, sourceFile) => {
  const moduleSymbol = safe(() => project.checker.getSymbolAtLocation(sourceFile), undefined);
  if (!moduleSymbol) return [];
  return safe(() => project.checker.getExportsOfModule(moduleSymbol), []).map((symbol) =>
    resolveAlias(project.checker, symbol),
  );
};

const inferredExportTypes = (project, sourceFile) =>
  moduleExports(project, sourceFile).flatMap((symbol) =>
    declarationsForSymbol(project, symbol)
      .filter((declaration) => declaration.getSourceFile() === sourceFile)
      .flatMap((declaration) => inferredDeclarationTypes(project, declaration)),
  );

const isProjectDeclaration = (project, declaration) =>
  isProjectSource(project, declaration.getSourceFile());

const namedTypeDeclarations = (project, type) => {
  const symbol = type.getSymbol();
  if (!symbol || (symbol.flags & NAMED_TYPE_FLAGS) === 0) return [];
  return declarationsForSymbol(project, resolveAlias(project.checker, symbol)).filter(
    (declaration) => isNamedEmitTarget(declaration) && isProjectDeclaration(project, declaration),
  );
};

const isLibrarySymbol = (project, symbol) => {
  const declarations = declarationsForSymbol(project, symbol);
  return (
    declarations.length > 0 &&
    declarations.every((declaration) => !isProjectDeclaration(project, declaration))
  );
};

// A symbol of a library, or a project symbol that its module exports.
const isImportable = (project, exportedSymbols, symbol) => {
  if (isLibrarySymbol(project, symbol)) return true;
  return declarationsForSymbol(project, symbol).some((declaration) =>
    exportedSymbols(declaration.getSourceFile()).has(symbol),
  );
};

const isImportableValueType = (project, exportedSymbols, symbol) =>
  Boolean(symbol) &&
  (symbol.flags & VALUE_TYPE_FLAGS) !== 0 &&
  isImportable(project, exportedSymbols, symbol);

const signatureChildren = (project, type) =>
  [SignatureKind.Call, SignatureKind.Construct].flatMap((kind) =>
    safe(() => project.checker.getSignaturesOfType(type, kind), []).flatMap((signature) => [
      safe(() => project.checker.getReturnTypeOfSignature(signature), undefined),
      ...signature
        .getParameters()
        .map((parameter) => safe(() => project.checker.getTypeOfSymbol(parameter), undefined)),
    ]),
  );

const propertyChildren = (project, type) =>
  safe(() => project.checker.getPropertiesOfType(type), []).map((property) =>
    safe(() => project.checker.getTypeOfSymbol(property), undefined),
  );

const typeArguments = (project, type) => [
  ...safe(() => type.getTypes(), []),
  ...(type.isTypeReference() ? safe(() => project.checker.getTypeArguments(type), []) : []),
];

/** The types that declaration emit writes inside `type`. */
const emitChildren = (project, exportedSymbols, type) => {
  if (type.isTypeParameter()) {
    return [safe(() => project.checker.getConstraintOfTypeParameter(type), undefined)];
  }
  // Emit writes the name of an alias that the code can import, and the
  // structure of any other alias.
  const alias = type.getAliasSymbol();
  if (alias && isImportable(project, exportedSymbols, alias)) {
    return safe(() => type.getAliasTypeArguments(), []);
  }
  const symbol = type.getSymbol();
  if (isImportableValueType(project, exportedSymbols, symbol)) return [];
  const named =
    namedTypeDeclarations(project, type).length > 0 ||
    Boolean(symbol && (symbol.flags & NAMED_TYPE_FLAGS) !== 0 && isLibrarySymbol(project, symbol));
  // Emit writes only the name and the type arguments of a named type, for
  // example `ZodMiniObject<{ entry: ZodMiniType<Entry> }>`.
  if (named) return typeArguments(project, type);
  return [
    ...typeArguments(project, type),
    ...safe(() => type.getAliasTypeArguments(), []),
    ...signatureChildren(project, type),
    ...propertyChildren(project, type),
  ];
};

const isAliasedTypeHidden = (project, exportedSymbols, type) => {
  const alias = type.getAliasSymbol();
  return Boolean(alias && isImportable(project, exportedSymbols, alias));
};

const escapeRegExp = (text) => text.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");

const SOURCE_EXTENSION = /\.(?:d\.)?[cm]?[jt]sx?$/u;

// The last specifier segments that can name the module of `fileName`:
// `./schemas` and `./schemas.js` for `schemas.ts`, `./dom` and `./dom/index`
// for `dom/index.ts`.
const moduleStems = (fileName) => {
  const stem = path.basename(fileName).replace(SOURCE_EXTENSION, "");
  return stem === "index" ? [stem, path.basename(path.dirname(fileName))] : [stem];
};

/**
 * No file names a target, so no file has it in scope, and emit must write
 * it with an import type: `import("./schemas").Entry`. The pattern matches
 * that text for the module and the name of the target. Emit can also write
 * `schemas.Entry` through a namespace import. The pattern misses that form,
 * so the type keeps its finding.
 */
const emitNamePattern = ({ name, fileName }) => {
  const stems = moduleStems(fileName).map(escapeRegExp).join("|");
  return new RegExp(
    `[/"](?:${stems})(?:\\.[cm]?[jt]sx?)?"\\)\\.${escapeRegExp(name)}(?![\\p{ID_Continue}$])`,
    "u",
  );
};

/**
 * One checker call prints the type as declaration emit writes it. Emit can
 * name a target only when this text matches the pattern of the target, so
 * the walk, with its many checker calls, runs only for these types. The walk
 * then decides by the exact declaration, never by the text.
 */
const mayNameTarget = (project, { type, holder }, patterns) => {
  const text = safe(
    () => project.checker.typeToString(type, holder, NodeBuilderFlags.NoTruncation),
    undefined,
  );
  if (text === undefined) return true;
  return patterns.some((pattern) => pattern.test(text));
};

/**
 * Walks the inferred types of the exported declarations of `sourceFile` and
 * calls `onTarget` with each project interface of another file that emit
 * names, and with the exported declaration that holds it.
 */
const walkFile = (project, exportedSymbols, sourceFile, patterns, onTarget) => {
  const pending = inferredExportTypes(project, sourceFile).filter((item) =>
    mayNameTarget(project, item, patterns),
  );
  const seen = new Set();
  while (pending.length > 0 && seen.size < TYPE_BUDGET_PER_FILE) {
    const { type, holder } = pending.pop();
    if (!type || seen.has(type.id)) continue;
    seen.add(type.id);
    if (!isAliasedTypeHidden(project, exportedSymbols, type)) {
      namedTypeDeclarations(project, type)
        .filter((declaration) => declaration.getSourceFile() !== sourceFile)
        .forEach((declaration) => onTarget(declaration, holder));
    }
    pending.push(...held(holder, emitChildren(project, exportedSymbols, type)));
  }
};

const exportedSymbolsOf = (project) => {
  const byFile = new Map();
  return (sourceFile) => {
    const cached = byFile.get(sourceFile);
    if (cached) return cached;
    const symbols = new Set(moduleExports(project, sourceFile));
    byFile.set(sourceFile, symbols);
    return symbols;
  };
};

const moduleSpecifiers = (sourceFile) =>
  sourceFile.statements.flatMap((statement) =>
    (isImportDeclaration(statement) || isExportDeclaration(statement)) && statement.moduleSpecifier
      ? [statement.moduleSpecifier]
      : [],
  );

/**
 * The files that import a file of `targetFiles`, directly or through other
 * files, and the target files. An inferred type reaches another file only
 * through an import, so only these files can hold a target.
 */
const importClosure = (project, targetFiles) => {
  const importers = new Map();
  for (const sourceFile of projectSourceFiles(project)) {
    const specifiers = moduleSpecifiers(sourceFile);
    if (specifiers.length === 0) continue;
    const modules = safe(() => project.checker.getSymbolAtLocation(specifiers), []);
    modules.forEach((moduleSymbol) =>
      declarationsForSymbol(project, moduleSymbol).forEach((declaration) => {
        const imported = declaration.getSourceFile().fileName;
        const list = importers.get(imported) ?? [];
        list.push(sourceFile.fileName);
        importers.set(imported, list);
      }),
    );
  }
  const closure = new Set(targetFiles);
  const pending = [...targetFiles];
  while (pending.length > 0) {
    for (const importer of importers.get(pending.pop()) ?? []) {
      if (closure.has(importer)) continue;
      closure.add(importer);
      pending.push(importer);
    }
  }
  return closure;
};

const recordEmitUse = (state, entry, use) => {
  const id = entry.query.id;
  state.totalByQuery.set(id, state.totalByQuery.get(id) + 1);
  state.directFilesByQuery.get(id).add(use.file);
  recordBoundedUse(
    state.evidenceByQuery.get(id),
    use.evidence,
    entry.query.operation,
    state.evidenceLimit,
  );
};

// The type queries that the reference scan found no use for.
const emitTargets = (root, state, resolvedQueries, locate) => {
  const targets = new Map();
  resolvedQueries
    .filter(
      ({ query }) =>
        query.operation === "symbol-use" &&
        query.symbol.namespace === "type" &&
        state.totalByQuery.get(query.id) === 0,
    )
    .forEach((entry) => {
      const record = (holder) =>
        recordEmitUse(state, entry, {
          file: sourceFileIdentity(holder.getSourceFile()),
          evidence: {
            ...locate(root, holder.name ?? holder),
            role: "declaration-emit",
            source: "checker",
            namespace: "type",
            via: [],
          },
        });
      entry.resolved.ownerContexts.forEach(({ declaration }) =>
        targets.set(stableDeclarationKey(declaration, "type"), {
          name: declarationName(declaration),
          fileName: declaration.getSourceFile().fileName,
          record,
        }),
      );
    });
  return targets;
};

/**
 * Credits the type queries without a use whose declaration emit another file
 * needs. No file names such a type, but an inferred type of an export in
 * another file contains it, so emit of that file writes its name. Each credit
 * is a `declaration-emit` evidence item at the holding declaration, at most
 * one for each file. Only projects that emit declaration files take part.
 *
 * `locate` gives the protocol location of a node.
 */
export const recordDeclarationEmitUses = (root, projects, state, resolvedQueries, locate) => {
  const targets = emitTargets(root, state, resolvedQueries, locate);
  if (targets.size === 0) return;
  const patterns = [...targets.values()].filter(({ name }) => name).map(emitNamePattern);
  const targetFiles = [...targets.values()].map(({ fileName }) => fileName);
  const scannedFiles = new Set();
  for (const project of projects) {
    if (!emitsDeclarations(root, project)) continue;
    const exportedSymbols = exportedSymbolsOf(project);
    const closure = importClosure(project, targetFiles);
    for (const sourceFile of projectSourceFiles(project)) {
      if (!closure.has(sourceFile.fileName) || scannedFiles.has(sourceFile.fileName)) continue;
      scannedFiles.add(sourceFile.fileName);
      const reported = new Set();
      walkFile(project, exportedSymbols, sourceFile, patterns, (declaration, holder) => {
        const key = stableDeclarationKey(declaration, "type");
        const target = targets.get(key);
        if (!target || reported.has(key)) return;
        reported.add(key);
        target.record(holder);
      });
    }
  }
};
