### Fixed

- **The fallow skill puts the audit styling fields under `complexity`.** The MCP reference said that `styling_findings`, `css_analytics` and `styling_health` are under the health sub-result. The `fallow audit` JSON has no `health` key: these fields are in the nested `complexity` block.
