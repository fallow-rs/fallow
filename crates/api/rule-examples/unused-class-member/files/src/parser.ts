export class Parser {
  parse(input: string): number {
    return Number(input.replace("ORDER-", ""));
  }

  parseLegacy(input: string): number {
    return Number(input.slice(3));
  }
}
