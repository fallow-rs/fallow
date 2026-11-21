// A small Rust lexer for the repository checks. It knows comments, string
// literals and char literals, so that a check can ignore text inside them.

const WORD_CHAR = /\w/;

const skipQuoted = (source, start, quote) => {
  let index = start + 1;
  while (index < source.length && source[index] !== quote) {
    index += source[index] === "\\" ? 2 : 1;
  }
  return index + 1;
};

const blank = (text) => text.replace(/[^\n]/g, " ");

const commentEnd = (source, index) => {
  if (source[index] !== "/") return null;
  if (source[index + 1] === "/") {
    const end = source.indexOf("\n", index);
    return end === -1 ? source.length : end;
  }
  if (source[index + 1] !== "*") return null;
  const end = source.indexOf("*/", index + 2);
  return end === -1 ? source.length : end + 2;
};

const stringEnd = (source, index) =>
  source[index] === '"' ? skipQuoted(source, index, '"') : null;

const RAW_STRING_START = /^b?r(#*)"/;

const prefixedStringEnd = (source, index) => {
  if (WORD_CHAR.test(source[index - 1] ?? "")) return null;
  if (source.startsWith('b"', index)) return skipQuoted(source, index + 1, '"');
  const raw = RAW_STRING_START.exec(source.slice(index, index + 260));
  if (!raw) return null;
  const end = source.indexOf(`"${raw[1]}`, index + raw[0].length);
  return end === -1 ? source.length : end + raw[1].length + 1;
};

const CHAR_LITERAL = /^'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]+\}|.)|[^\\'\n])'/u;

const charEnd = (source, index) => {
  if (source[index] !== "'") return null;
  const literal = CHAR_LITERAL.exec(source.slice(index, index + 12));
  return literal ? index + literal[0].length : null;
};

const LITERAL_SCANNERS = [commentEnd, stringEnd, prefixedStringEnd, charEnd];

const literalEnd = (source, index) => {
  for (const scan of LITERAL_SCANNERS) {
    const end = scan(source, index);
    if (end !== null) return end;
  }
  return null;
};

/**
 * Replaces comments, string literals and char literals with spaces. The
 * result has the same length and line breaks as the input, so an offset in
 * the result is the same offset in the source.
 */
export const stripLiterals = (source) => {
  let output = "";
  let index = 0;
  while (index < source.length) {
    const end = literalEnd(source, index);
    if (end === null) {
      output += source[index];
      index += 1;
    } else {
      output += blank(source.slice(index, end));
      index = end;
    }
  }
  return output;
};
