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
		-- Every issue type is enabled by default. List only the ones you
		-- want to turn off; any key you omit stays enabled.
		issueTypes = {
			["circular-dependencies"] = false,
		},
	},
})

vim.lsp.enable("fallow")
```

`init_options` is optional; `cmd`, `filetypes`, and `root_markers` alone are enough to attach. Fallow reads issue toggles from LSP initialization options: set an issue type to `false` to disable it in editor diagnostics without changing your project config. The full list of keys matches Fallow's issue types (kebab-case); a client can fetch the live catalog via the custom `fallow/issueTypes` request.

Diagnostics are delivered through the LSP 3.17 pull model and refreshed on save. The first analysis runs when the server attaches, so a freshly opened buffer shows findings once the initial pass completes (or after the next save), not necessarily the instant the file opens.

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
