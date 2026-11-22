### Fixed

- **`fallow audit` on a sparse checkout now builds the base snapshot from the same files as the checkout.** Git cone mode also checks out the files directly inside each parent directory of a cone, such as `apps/package.json` for the cone `apps/web`. The base snapshot now keeps these files too, so a parent manifest or ignore file no longer shows as present at head and missing at base.
- **`fallow audit` rebuilds the reusable base snapshot after a sparse cone change.** The snapshot was reused when only the cone changed at the same base commit, so the base held the old set of files. The snapshot identity now includes the sparse cone and the analysis subdirectory.
