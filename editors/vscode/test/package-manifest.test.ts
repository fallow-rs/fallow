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

interface ViewContribution {
  readonly id: string;
  readonly name: string;
}

interface ViewsWelcomeContribution {
  readonly view: string;
  readonly contents: string;
  readonly when?: string;
}

interface ConfigProperty {
  readonly description?: string;
  readonly markdownDescription?: string;
  readonly default?: unknown;
}

interface ExtensionPackage {
  readonly contributes: {
    readonly commands: readonly CommandContribution[];
    readonly configuration: {
      readonly properties: Record<string, ConfigProperty>;
    };
    readonly views: {
      readonly fallow: readonly ViewContribution[];
    };
    readonly viewsWelcome: readonly ViewsWelcomeContribution[];
    readonly menus: {
      readonly "view/title": readonly MenuContribution[];
      readonly commandPalette: readonly MenuContribution[];
    };
  };
}

const pkg = JSON.parse(
  readFileSync(resolve(__dirname, "../package.json"), "utf8"),
) as ExtensionPackage;
const configKeysSource = readFileSync(resolve(__dirname, "../src/configKeys.ts"), "utf8");
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
    expect(configKeysSource).toContain('"fallow.autoDownload"');
  });
});

describe("package.json duplication settings", () => {
  it("contributes every duplication knob used by sidebar analysis", () => {
    const properties = pkg.contributes.configuration.properties;

    for (const key of [
      "fallow.duplication.mode",
      "fallow.duplication.threshold",
      "fallow.duplication.minTokens",
      "fallow.duplication.minLines",
      "fallow.duplication.minOccurrences",
      "fallow.duplication.skipLocal",
      "fallow.duplication.crossLanguage",
      "fallow.duplication.ignoreImports",
    ]) {
      expect(properties[key]?.description).toBeTruthy();
    }
  });
});

describe("package.json duplication settings", () => {
  it("contributes the sidebar duplication filter settings", () => {
    const properties = pkg.contributes.configuration.properties;

    expect(properties["fallow.duplication.mode"]).toBeDefined();
    expect(properties["fallow.duplication.threshold"]).toBeDefined();
    expect(properties["fallow.duplication.minLines"]).toBeDefined();
    expect(properties["fallow.duplication.minOccurrences"]).toBeDefined();
  });

  it("restarts and reruns analysis when duplication settings change", () => {
    expect(configKeysSource).toContain('"fallow.duplication"');
  });
});

describe("package.json security candidates contributions", () => {
  const securityView = pkg.contributes.views.fallow.find((view) => view.id === "fallow.security");
  const securityWelcome = pkg.contributes.viewsWelcome.filter(
    (entry) => entry.view === "fallow.security",
  );
  const securitySetting = pkg.contributes.configuration.properties["fallow.security.enabled"];

  it("contributes the Security Candidates view", () => {
    expect(securityView).toMatchObject({ name: "Security Candidates" });
  });

  it("contributes the scan command with a shield icon", () => {
    expect(command("fallow.analyzeSecurity")).toMatchObject({
      title: "Fallow: Scan for Security Candidates",
      icon: "$(shield)",
    });
  });

  it("contributes both view/title menu states for the scan command", () => {
    const entries = pkg.contributes.menus["view/title"].filter(
      (entry) => entry.command === "fallow.analyzeSecurity",
    );
    expect(entries.map((entry) => entry.when)).toEqual([
      "view == fallow.security && !fallow.hasAnalyzedSecurity",
      "view == fallow.security && fallow.hasAnalyzedSecurity",
    ]);
  });

  it("contributes an opt-in setting defaulting to false", () => {
    expect(securitySetting?.default).toBe(false);
    expect(securitySetting?.markdownDescription).toBeTruthy();
  });

  it("frames every security string as a candidate, never a confirmed vulnerability", () => {
    const strings = [
      securityView?.name ?? "",
      command("fallow.analyzeSecurity")?.title ?? "",
      securitySetting?.markdownDescription ?? "",
      ...securityWelcome.map((entry) => entry.contents),
    ].filter((value) => value.length > 0);

    expect(strings.length).toBeGreaterThan(0);

    for (const value of strings) {
      const lower = value.toLowerCase();
      // Every surface must name them as candidates.
      expect(lower).toContain("candidate");
      // "vulnerabilit"/"confirmed" may only appear in honest negations
      // ("never confirmed vulnerabilities", "NOT verified vulnerabilities");
      // a positive claim that these ARE vulnerabilities/confirmed is forbidden.
      if (lower.includes("vulnerabilit") || lower.includes("confirmed")) {
        const negated = /\b(?:never|not|un\w+|no)\b/.test(lower);
        expect(negated, `unframed security claim: ${value}`).toBe(true);
      }
    }
  });
});

describe("package.json license commands", () => {
  it("contributes the four license commands and registers each in extension.ts", () => {
    for (const id of [
      "fallow.license.activate",
      "fallow.license.status",
      "fallow.license.refresh",
      "fallow.license.deactivate",
    ]) {
      expect(command(id)?.title).toMatch(/^Fallow: /);
      expect(extensionSource).toContain(`registerCommand("${id}"`);
    }
  });

  it("documents both opt-out / opt-in license settings", () => {
    const properties = pkg.contributes.configuration.properties;
    expect(properties["fallow.license.showStatusBar"]?.description).toBeTruthy();
    expect(properties["fallow.license.refreshOnStartup"]?.description).toBeTruthy();
  });

  it("keeps the startup probe off by default (does not shell out on activation)", () => {
    const properties = pkg.contributes.configuration.properties as Record<
      string,
      { readonly default?: unknown }
    >;
    expect(properties["fallow.license.refreshOnStartup"]?.default).toBe(false);
    expect(properties["fallow.license.showStatusBar"]?.default).toBe(true);
  });
});
