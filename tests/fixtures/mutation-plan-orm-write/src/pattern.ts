import pg from "pg";

const DIGITS = /(\d+)/;

export const pool = new pg.Pool();

export function parseId(text: string): number {
  const match = /(\d+)/.exec(text);
  return match === null ? -1 : Number(match[1]) + 1;
}

export function parseWord(text: string): number {
  const match = new RegExp("[a-z]+").exec(text);
  return match === null ? 0 : match[0].length * 2;
}

export function parseStored(text: string): number {
  const match = DIGITS.exec(text);
  return match === null ? 0 : match[1].length - 1;
}
