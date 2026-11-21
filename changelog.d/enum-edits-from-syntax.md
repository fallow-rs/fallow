### Fixed

- **`fallow fix` keeps the other enum members of a line.** For `enum E {\n  A, B,\n  C\n}` with `A` unused, the fix deleted the whole line and lost `B`. It now removes only `A` and its comma.
- **`fallow fix` keeps a comma inside a string value of an enum.** For `enum E { A = "x,y", B }` with `B` unused, the fix split the line on commas and wrote `"x, y"`. It now removes only `, B`, and each other byte stays.
- **`fallow fix` removes an enum member initializer over several lines.** The fix deleted only the first line of the member and left the rest of the initializer.

### Changed

- **`fallow fix` removes an enum member only in a file that parses without an error.** The fix reads the members from the syntax tree. In a file with a syntax error the finding stays in the report.
