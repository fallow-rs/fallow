import path from "node:path";

import { SymbolFlags } from "typescript/unstable/sync";
import {
  isClassDeclaration,
  isClassExpression,
  isEnumDeclaration,
  isExportSpecifier,
  isFunctionDeclaration,
  isGetAccessorDeclaration,
  isInterfaceDeclaration,
  isMethodDeclaration,
  isMethodSignatureDeclaration,
  isModuleDeclaration,
  isNamespaceExport,
  isPropertyDeclaration,
  isPropertySignatureDeclaration,
  isSetAccessorDeclaration,
  isSourceFile,
  isTypeAliasDeclaration,
  isVariableDeclaration,
} from "typescript/unstable/ast/is";

import { canonicalFileIdentity } from "./file-identity.mjs";

const DECLARATION_KIND_RULES = [
  [[isClassDeclaration, isClassExpression], "class"],
  [[isInterfaceDeclaration], "interface"],
  [[isTypeAliasDeclaration], "type_alias"],
  [[isEnumDeclaration], "enum"],
  [[isFunctionDeclaration], "function"],
  [[isModuleDeclaration], "namespace"],
  [[isVariableDeclaration], "variable"],
  [
    [
      isMethodDeclaration,
      isMethodSignatureDeclaration,
      isGetAccessorDeclaration,
      isSetAccessorDeclaration,
    ],
    "class_method",
  ],
  [[isPropertyDeclaration, isPropertySignatureDeclaration], "class_property"],
];
const TYPE_ONLY_DECLARATIONS = [
  isInterfaceDeclaration,
  isTypeAliasDeclaration,
  isPropertySignatureDeclaration,
];
const DUAL_NAMESPACE_DECLARATIONS = [
  isClassDeclaration,
  isClassExpression,
  isEnumDeclaration,
  isSourceFile,
];
const DECLARATION_OWNER_NODES = [isClassDeclaration, isClassExpression, isInterfaceDeclaration];

const slash = (value) => value.split(path.sep).join("/");
const matchesAny = (node, checks) => checks.some((check) => check(node));

export const relativePath = (root, fileName) =>
  slash(path.relative(canonicalFileIdentity(root), canonicalFileIdentity(fileName)));

export const sourceFileIdentity = (sourceFile) => canonicalFileIdentity(sourceFile.fileName);

export const positions = (node) => {
  const sourceFile = node.getSourceFile();
  const nodes = node.name ? [node, node.name] : [node];
  return nodes.map((candidate) => {
    const start = candidate.getStart(sourceFile);
    const { line } = sourceFile.getLineAndCharacterOfPosition(start);
    const lineStart = sourceFile.getPositionOfLineAndCharacter(line, 0);
    return {
      line: line + 1,
      col: Buffer.byteLength(sourceFile.text.slice(lineStart, start), "utf8"),
    };
  });
};

export const nodeText = (node) => node?.text;

export const declarationKind = (node) =>
  DECLARATION_KIND_RULES.find(([checks]) => matchesAny(node, checks))?.[1];

export const declarationNamespaces = (node) =>
  matchesAny(node, TYPE_ONLY_DECLARATIONS)
    ? new Set(["type"])
    : new Set(matchesAny(node, DUAL_NAMESPACE_DECLARATIONS) ? ["type", "value"] : ["value"]);

export const declarationName = (node) => node.name?.text;

const ownerName = (node) => {
  if (!node) return null;
  if (matchesAny(node, DECLARATION_OWNER_NODES)) return nodeText(node.name) ?? null;
  return ownerName(node.parent);
};

export const declarationOwner = (node) => ownerName(node.parent);

export const ownerDeclaration = (node) => {
  let current = node?.parent;
  while (current) {
    if (matchesAny(current, DECLARATION_OWNER_NODES)) return current;
    current = current.parent;
  }
  return undefined;
};

export const isDeclaration = (node) => declarationKind(node) !== undefined;

export const isExportAnchor = (node) => isExportSpecifier(node) || isNamespaceExport(node);

const visit = (node, callback) => {
  callback(node);
  node.forEachChild((child) => {
    visit(child, callback);
    return undefined;
  });
};

const declarationIndexes = new WeakMap();

const anchorKey = ({ declarationKind: kind, localName, exportedName, owner, line, col }) =>
  [kind, localName, exportedName, owner ?? "", line, col].join("\0");

const setExportAnchor = (index, node, localName, exportedName, owner, position) => {
  index.set(
    anchorKey({
      declarationKind: "export",
      localName,
      exportedName,
      owner,
      ...position,
    }),
    node,
  );
};

const exportSpecifierNames = (node) => {
  const exportedName = nodeText(node.name);
  return {
    exportedName,
    localName: nodeText(node.propertyName) ?? exportedName,
  };
};

const exportSpecifierPositions = (node) =>
  [node, node.name, node.propertyName].filter(Boolean).flatMap((candidate) => positions(candidate));

const indexExportPosition = (index, node, names, position) => {
  for (const localName of new Set([names.localName, names.exportedName])) {
    setExportAnchor(index, node, localName, names.exportedName, null, position);
  }
};

const indexExportSpecifier = (index, node) => {
  const names = exportSpecifierNames(node);
  exportSpecifierPositions(node).forEach((position) =>
    indexExportPosition(index, node, names, position),
  );
};

const indexNamespaceExport = (index, node) => {
  const exportedName = nodeText(node.name);
  if (!exportedName) return;
  [node.parent, node]
    .flatMap((candidate) => positions(candidate))
    .forEach((position) =>
      indexExportPosition(index, node, { localName: exportedName, exportedName }, position),
    );
};

const defaultModifierFor = (node) =>
  node.modifiers?.find((modifier) => modifier.getText(node.getSourceFile()) === "default");

const indexNamedDeclaration = (index, node, localName) => {
  for (const position of positions(node)) {
    const common = {
      localName,
      exportedName: localName,
      owner: declarationOwner(node),
      ...position,
    };
    index.set(anchorKey({ declarationKind: declarationKind(node), ...common }), node);
    index.set(anchorKey({ declarationKind: "export", ...common }), node);
  }
};

const indexDefaultDeclaration = (index, node, localName, defaultModifier) => {
  if (!defaultModifier) return;
  for (const position of [...positions(node), ...positions(defaultModifier)]) {
    for (const identityLocalName of new Set([localName, "default"])) {
      setExportAnchor(index, node, identityLocalName, "default", declarationOwner(node), position);
    }
  }
};

const declarationLocalName = (node, defaultModifier) =>
  declarationName(node) ?? (defaultModifier ? "default" : undefined);

const indexDeclarationNode = (index, node) => {
  if (!isDeclaration(node)) return;
  const defaultModifier = defaultModifierFor(node);
  const localName = declarationLocalName(node, defaultModifier);
  if (!localName) return;
  indexNamedDeclaration(index, node, localName);
  indexDefaultDeclaration(index, node, localName, defaultModifier);
};

const declarationIndex = (sourceFile) => {
  const cached = declarationIndexes.get(sourceFile);
  if (cached) return cached;
  const index = new Map();
  visit(sourceFile, (node) => {
    if (isExportSpecifier(node)) {
      indexExportSpecifier(index, node);
      return;
    }
    if (isNamespaceExport(node)) {
      indexNamespaceExport(index, node);
      return;
    }
    indexDeclarationNode(index, node);
  });
  declarationIndexes.set(sourceFile, index);
  return index;
};

export const findDeclaration = (sourceFile, identity) => {
  const anchor = declarationIndex(sourceFile).get(anchorKey(identity));
  if (!anchor || isExportAnchor(anchor)) return anchor;
  return declarationNamespaces(anchor).has(identity.namespace) ? anchor : undefined;
};

export const symbolForDeclaration = (project, declaration) =>
  project.checker.getSymbolAtLocation(declaration.name ?? declaration);

export const resolveAlias = (checker, symbol) => {
  if (!symbol) return undefined;
  const seen = new Set();
  let current = symbol;
  while ((current.flags & SymbolFlags.Alias) !== 0 && !seen.has(current)) {
    seen.add(current);
    const aliased = checker.getAliasedSymbol(current);
    if (checker.isUnknownSymbol(aliased) || aliased === current) return current;
    current = aliased;
  }
  return current;
};

export const declarationsForSymbol = (project, symbol) =>
  (symbol?.declarations ?? []).map((handle) => handle.resolve(project)).filter(Boolean);

export const stableDeclarationKey = (node, namespace = "value") => {
  const first = positions(node)[0];
  return [
    sourceFileIdentity(node.getSourceFile()),
    namespace,
    declarationKind(node) ?? "unknown",
    declarationName(node) ?? "default",
    first.line,
    first.col,
    declarationOwner(node) ?? "",
  ].join("\0");
};

export const stableSymbolIdentity = (root, declaration, namespace, exportedName) => {
  const position = positions(declaration)[0];
  return {
    path: relativePath(root, declaration.getSourceFile().fileName),
    namespace,
    declaration_kind: declarationKind(declaration) ?? "unknown",
    exported_name: exportedName,
    local_name: declarationName(declaration) ?? exportedName,
    line: position.line,
    col: position.col,
    owner: declarationOwner(declaration),
  };
};

export const semanticQueryIdentity = (root, query, resolved) => {
  if (query.symbol.declarationKind !== "export") {
    return stableSymbolIdentity(
      root,
      resolved.declaration,
      query.symbol.namespace,
      query.symbol.exportedName,
    );
  }
  return {
    path: relativePath(root, query.symbol.absolutePath),
    namespace: query.symbol.namespace,
    declaration_kind: query.symbol.declarationKind,
    exported_name: query.symbol.exportedName,
    local_name: query.symbol.localName,
    line: query.symbol.line,
    col: query.symbol.col,
    owner: query.symbol.owner,
  };
};

export const isProjectSource = (project, sourceFile) =>
  Boolean(sourceFile) &&
  !sourceFile.isDeclarationFile &&
  !project.program.isSourceFileDefaultLibrary(sourceFile) &&
  !project.program.isSourceFileFromExternalLibrary(sourceFile);

export const projectSourceFiles = (project) =>
  project.program
    .getSourceFileNames()
    .map((fileName) => project.program.getSourceFile(fileName))
    .filter((sourceFile) => isProjectSource(project, sourceFile));

const projectExportIndexes = new WeakMap();

const exportedDeclarations = (project, sourceFile) => {
  const moduleSymbol = project.checker.getSymbolAtLocation(sourceFile);
  if (!moduleSymbol) return [];
  return project.checker.getExportsOfModule(moduleSymbol).flatMap((exported) => {
    const target = resolveAlias(project.checker, exported);
    return declarationsForSymbol(project, target).flatMap((declaration) =>
      [...declarationNamespaces(declaration)].map((namespace) => ({
        exportedName: exported.name,
        declaration,
        namespace,
      })),
    );
  });
};

const exportIndexKey = ({ exportedName, declaration, namespace }) =>
  `${exportedName}\0${stableDeclarationKey(declaration, namespace)}`;

export const projectExportIndex = (project) => {
  const cached = projectExportIndexes.get(project);
  if (cached) return cached;
  const index = new Set(
    projectSourceFiles(project).flatMap((sourceFile) =>
      exportedDeclarations(project, sourceFile).map(exportIndexKey),
    ),
  );
  projectExportIndexes.set(project, index);
  return index;
};
