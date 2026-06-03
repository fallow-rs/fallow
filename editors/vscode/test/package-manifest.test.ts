import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

interface CommandContribution {
  readonly command: string;
  readonly title: string;
  readonly icon?: string;
}

interface MenuContribution {
  readonly command: string;
  readonly when?: string;
  readonly group?: string;
}

interface ExtensionPackage {
  readonly contributes: {
    readonly commands: readonly CommandContribution[];
    readonly configuration: {
      readonly properties: Record<
        string,
        {
          readonly description?: string;
        }
      >;
    };
    readonly menus: {
      readonly "view/title": readonly MenuContribution[];
      readonly commandPalette: readonly MenuContribution[];
    };
  };
}

const pkg = JSON.parse(
  readFileSync(resolve(__dirname, "../package.json"), "utf8"),
) as ExtensionPackage;
const extensionSource = readFileSync(resolve(__dirname, "../src/extension.ts"), "utf8");

const command = (id: string): CommandContribution | undefined =>
  pkg.contributes.commands.find((entry) => entry.command === id);

const viewTitleCommand = (id: string): MenuContribution | undefined =>
  pkg.contributes.menus["view/title"].find((entry) => entry.command === id);

const commandPaletteEntry = (id: string): MenuContribution | undefined =>
  pkg.contributes.menus.commandPalette.find((entry) => entry.command === id);

describe("package.json command contributions", () => {
  it("uses search only for the initial analysis action", () => {
    expect(command("fallow.analyze")).toMatchObject({
      title: "Fallow: Run Analysis",
      icon: "$(search)",
    });
  });

  it("uses a refresh icon for the post-analysis reload action", () => {
    expect(command("fallow.reloadAnalysis")).toMatchObject({
      title: "Fallow: Reload Analysis",
      icon: "$(refresh)",
    });
  });
});

describe("package.json view title menus", () => {
  it("shows run analysis before results are loaded", () => {
    expect(viewTitleCommand("fallow.analyze")).toMatchObject({
      when: "(view == fallow.deadCode || view == fallow.duplicates) && !fallow.hasAnalyzed",
      group: "navigation",
    });
  });

  it("shows reload analysis after results are loaded", () => {
    expect(viewTitleCommand("fallow.reloadAnalysis")).toMatchObject({
      when: "(view == fallow.deadCode || view == fallow.duplicates) && fallow.hasAnalyzed",
      group: "navigation",
    });
  });

  it("keeps the reload command out of the command palette", () => {
    expect(commandPaletteEntry("fallow.reloadAnalysis")).toMatchObject({
      when: "false",
    });
    expect(commandPaletteEntry("fallow.analyze")).toBeUndefined();
  });
});

describe("package.json binary download settings", () => {
  it("documents that auto-download manages both binaries", () => {
    const description =
      pkg.contributes.configuration.properties["fallow.autoDownload"]?.description ?? "";

    expect(description).toContain("fallow-lsp");
    expect(description).toContain("fallow CLI");
  });

  it("restarts binary resolution when auto-download changes", () => {
    expect(extensionSource).toContain('"fallow.autoDownload"');
  });
});
