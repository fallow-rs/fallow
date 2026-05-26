// Control: a genuinely-unused non-remote module. Imported by nothing and not a
// framework entry point, so it MUST still report as unused-file. Proves the
// remote-function credit is scoped to *.remote.* and not applied project-wide.
export const orphanHelper = (): string => 'never used';
