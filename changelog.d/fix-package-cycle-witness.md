### Fixed

- **A deep package cycle always gets a `package-cycle` finding.** The cycle listing of a group of packages that import each other has a step budget. On a large layered group, the budget could run out before the search found one cycle, and the group then had no finding. The group now gets one shortest cycle through its first package, marked as truncated.
