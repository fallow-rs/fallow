/**
 * Minimal, zero-dependency JS/TS/JSON tokenizer for diff syntax highlighting.
 * Per-line and stateless (no cross-line block-comment / template tracking): a
 * pragmatic, robust approximation that never throws on odd input. Good enough to
 * give review diffs real syntax color without pulling in a highlighter.
 */
export type TokenType = "comment" | "string" | "number" | "keyword" | "plain";
export type Token = { type: TokenType; value: string };

const KEYWORDS = new Set([
  "abstract",
  "as",
  "async",
  "await",
  "break",
  "case",
  "catch",
  "class",
  "const",
  "continue",
  "debugger",
  "declare",
  "default",
  "delete",
  "do",
  "else",
  "enum",
  "export",
  "extends",
  "false",
  "finally",
  "for",
  "from",
  "function",
  "get",
  "if",
  "implements",
  "import",
  "in",
  "infer",
  "instanceof",
  "interface",
  "is",
  "keyof",
  "let",
  "namespace",
  "new",
  "null",
  "of",
  "package",
  "private",
  "protected",
  "public",
  "readonly",
  "return",
  "satisfies",
  "set",
  "static",
  "super",
  "switch",
  "this",
  "throw",
  "true",
  "try",
  "type",
  "typeof",
  "undefined",
  "var",
  "void",
  "while",
  "with",
  "yield",
]);

const NUMBER = /^(?:0[xXbBoO][0-9a-fA-F_]+|\d[\d_]*(?:\.\d+)?(?:[eE][+-]?\d+)?)/;
const IDENT = /^[A-Za-z_$][\w$]*/;

/** Tokenize one line of source into coalesced typed spans. */
export const tokenize = (line: string): Token[] => {
  const tokens: Token[] = [];
  const push = (type: TokenType, value: string): void => {
    const last = tokens[tokens.length - 1];
    if (last && last.type === type) last.value += value;
    else tokens.push({ type, value });
  };

  let i = 0;
  while (i < line.length) {
    const rest = line.slice(i);

    if (rest.startsWith("//")) {
      push("comment", rest);
      break;
    }
    if (rest.startsWith("/*")) {
      const end = rest.indexOf("*/");
      const frag = end === -1 ? rest : rest.slice(0, end + 2);
      push("comment", frag);
      i += frag.length;
      continue;
    }

    const ch = line[i] ?? "";
    if (ch === '"' || ch === "'" || ch === "`") {
      let j = i + 1;
      while (j < line.length) {
        if (line[j] === "\\") {
          j += 2;
          continue;
        }
        if (line[j] === ch) {
          j += 1;
          break;
        }
        j += 1;
      }
      push("string", line.slice(i, j));
      i = j;
      continue;
    }

    if (ch >= "0" && ch <= "9") {
      const m = NUMBER.exec(rest);
      if (m) {
        push("number", m[0]);
        i += m[0].length;
        continue;
      }
    }

    const id = IDENT.exec(rest);
    if (id) {
      push(KEYWORDS.has(id[0]) ? "keyword" : "plain", id[0]);
      i += id[0].length;
      continue;
    }

    push("plain", ch);
    i += 1;
  }
  return tokens;
};
