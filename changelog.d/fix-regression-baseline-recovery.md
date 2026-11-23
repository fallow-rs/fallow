### Fixed

- **Regression baseline errors give the save command for the selected analysis.** The command quotes file paths and keeps the selected project root, config file and type-aware override. For flag retirement and entry weights, the error tells you to move an incompatible file to a backup before you save a new file.
- **The regression breakdown compares only counts that the baseline can store.** For example, if a Vue project adds one unused file, the breakdown for a config baseline reports that file and omits existing unused component props. The total still includes all issue kinds.
