# `rule-pack-schema`: Rule Pack JSON Schema

Prints the JSON Schema for declarative rule pack files (the `rulePacks` config key), for editor autocomplete when authoring packs.

```bash
fallow rule-pack-schema > rule-pack-schema.json
```

Pack files can also reference the published schema directly: `"$schema": "https://raw.githubusercontent.com/fallow-rs/fallow/main/rule-pack-schema.json"`.
