import { vi } from "vitest";

export interface TestCommand {
  readonly command: string;
  readonly arguments: ReadonlyArray<unknown>;
}

export interface TestTreeItem {
  readonly label: string;
  readonly description?: string;
  readonly tooltip?: string;
  readonly contextValue?: string;
  readonly iconPath?: { readonly id: string };
  readonly collapsibleState: number;
  readonly command?: TestCommand;
}

export interface TestRange {
  readonly startLine: number;
  readonly startCharacter: number;
  readonly endLine: number;
  readonly endCharacter: number;
}

export const createTreeViewVscodeMock = (workspacePath: string): Record<string, unknown> => {
  class FakeTreeItem {
    public description: string | undefined;
    public tooltip: string | undefined;
    public contextValue: string | undefined;
    public command: unknown;
    public iconPath: unknown;

    public constructor(
      public readonly label: string,
      public readonly collapsibleState: number,
    ) {}
  }

  class FakeEventEmitter<T> {
    public readonly event = vi.fn();
    public fire = vi.fn((_value?: T) => {});
    public dispose = vi.fn();
  }

  class FakeRange {
    public constructor(
      public readonly startLine: number,
      public readonly startCharacter: number,
      public readonly endLine: number,
      public readonly endCharacter: number,
    ) {}
  }

  return {
    EventEmitter: FakeEventEmitter,
    Range: FakeRange,
    ThemeIcon: class {
      public constructor(public readonly id: string) {}
    },
    TreeItem: FakeTreeItem,
    TreeItemCollapsibleState: {
      None: 0,
      Collapsed: 1,
    },
    Uri: {
      file: (fsPath: string) => ({ fsPath }),
    },
    workspace: {
      workspaceFolders: [
        {
          uri: {
            fsPath: workspacePath,
          },
        },
      ],
    },
  };
};
