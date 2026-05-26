// Control file: not reachable from the public-api entry file and not imported
// anywhere. Must stay flagged as unused, proving the ng-package credit is scoped
// to the entry-file reachability graph rather than the whole project.
export function trulyUnused(): void {}
