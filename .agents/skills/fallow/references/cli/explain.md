# `explain`: Rule Explanation

Print rule rationale, examples, fix guidance, and docs URL for one issue type without running analysis.

## Usage

```bash
fallow explain unused-export
fallow explain fallow/code-duplication --format json --quiet
```

## Arguments

| Argument | Description |
|----------|-------------|
| `<issue-type>` | Issue type token or rule id, for example `unused-export`, `unused-exports`, `fallow/unused-dependency`, `high-complexity`, or `code-duplication`. |

## JSON Output Structure

```json
{
  "id": "fallow/unused-export",
  "name": "Unused Exports",
  "summary": "Export is never imported",
  "rationale": "Named exports that are never imported by any other module in the project. Includes both direct exports and re-exports through barrel files. The export may still be used locally within the same file.",
  "example": "export const formatPrice = ... exists in src/money.ts, but no module imports formatPrice.",
  "example_files": [
    {
      "path": "src/money.ts",
      "code": "export const formatTotal = (cents: number): string => `${(cents / 100).toFixed(2)} EUR`;\n\nexport const formatPrice = (cents: number): string => `EUR ${cents / 100}`;\n"
    }
  ],
  "example_base_files": [],
  "how_to_fix": "Remove the export or make it file-local. If it is public API, import it from an entry point or add an intentional suppression with context.",
  "docs": "https://fallow.tools/docs/rules/unused-export/"
}
```

`example_files` holds the files of a small project that makes the rule report a finding (the sample shows one of two files). `example_base_files` holds the files before the change, for a rule of `fallow audit`. Both are empty for a rule with no example project.

MCP equivalent: `fallow_explain` with required `issue_type`.
