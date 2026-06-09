// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";

export const OPEN_FILE_COMMAND = "fallow.openFile";

export interface OpenFileCommandArgs {
  readonly absolutePath: string;
  readonly line: number;
  readonly col: number;
  readonly endLine?: number;
  readonly endCol?: number;
}

export const openFileCommand = (
  absolutePath: string,
  line: number,
  col = 0,
  endLine = line,
  endCol = col,
): vscode.Command => ({
  command: OPEN_FILE_COMMAND,
  title: "Open File",
  arguments: [{ absolutePath, line, col, endLine, endCol }],
});

const isNumber = (value: unknown): value is number =>
  typeof value === "number" && Number.isFinite(value);

const isNonEmptyString = (value: unknown): value is string =>
  typeof value === "string" && value.length > 0;

const isOptionalNumber = (value: unknown): value is number | undefined =>
  value === undefined || isNumber(value);

const hasSelectionFields = (candidate: Partial<OpenFileCommandArgs>): boolean =>
  isNumber(candidate.line) &&
  isNumber(candidate.col) &&
  isOptionalNumber(candidate.endLine) &&
  isOptionalNumber(candidate.endCol);

const isOpenFileCommandArgs = (value: unknown): value is OpenFileCommandArgs => {
  if (value == null || typeof value !== "object") {
    return false;
  }
  const candidate = value as Partial<OpenFileCommandArgs>;
  return isNonEmptyString(candidate.absolutePath) && hasSelectionFields(candidate);
};

export const openFileCommandHandler = async (value: unknown): Promise<void> => {
  if (!isOpenFileCommandArgs(value)) {
    return;
  }

  const startLine = Math.max(0, value.line - 1);
  const startCol = Math.max(0, value.col);
  const endLine = Math.max(0, (value.endLine ?? value.line) - 1);
  const endCol = Math.max(0, value.endCol ?? value.col);

  await vscode.window.showTextDocument(vscode.Uri.file(value.absolutePath), {
    selection: new vscode.Range(startLine, startCol, endLine, endCol),
  });
};
