# Fallow for Neovim

Neovim configuration for [`fallow-lsp`](https://github.com/fallow-rs/fallow), the language server behind Fallow's editor diagnostics.

## What works

- diagnostics for unused files, exports, types, dependencies, enum/class members, unresolved imports, unlisted deps, duplicate exports, circular dependencies, and duplication
- hover information
- quick-fix code actions
- code lens where Neovim surfaces them

This setup is intentionally thin. It launches the existing `fallow-lsp` binary instead of re-implementing analysis logic inside the editor.

## Installation

Install Fallow globally so `fallow-lsp` is available on your `PATH`:

```sh
npm install -g fallow
```

Confirm Neovim can see the language server binary:

```sh
fallow-lsp --version
```

## Configuration

Add the language server to your Neovim config:

```lua
vim.lsp.config("fallow", {
	cmd = { "fallow-lsp" },
	filetypes = { "javascript", "typescript", "javascriptreact", "typescriptreact" },
	root_markers = { ".fallowrc.json", "package.json", ".git" },
	init_options = {
		editorInfo = {
			name = "Neovim",
			version = tostring(vim.version()),
		},
		issueTypes = {
			["unused-files"] = true,
			["unused-exports"] = true,
			["unused-types"] = true,
			["unused-dependencies"] = true,
			["unused-dev-dependencies"] = true,
			["unused-optional-dependencies"] = true,
			["unused-enum-members"] = true,
			["unused-class-members"] = true,
			["unresolved-imports"] = true,
			["unlisted-dependencies"] = true,
			["duplicate-exports"] = true,
			["type-only-dependencies"] = true,
			["circular-dependencies"] = true,
			["stale-suppressions"] = true,
		},
	},
})

vim.lsp.enable("fallow")
```

Fallow reads issue toggles from LSP initialization options. Set an issue type to `false` to disable it in editor diagnostics without changing your project config.

## Binary resolution

Neovim runs the `cmd` exactly as configured. If `fallow-lsp` is not on Neovim's `PATH`, point `cmd` at the absolute binary path:

```lua
vim.lsp.config("fallow", {
	cmd = { "/absolute/path/to/fallow-lsp" },
	filetypes = { "javascript", "typescript", "javascriptreact", "typescriptreact" },
	root_markers = { ".fallowrc.json", "package.json", ".git" },
})
```

## Development

1. Install Fallow globally with `npm install -g fallow`.
2. Add the config above to your Neovim setup.
3. Open a TypeScript or JavaScript project and run `:checkhealth vim.lsp`.
4. Confirm `fallow` is attached with `:lua vim.print(vim.lsp.get_clients({ name = "fallow" }))`.
