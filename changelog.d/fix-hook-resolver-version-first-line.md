### Fixed

- **The Git pre-commit hook of `fallow init --hooks` and `fallow hooks install --target git` prints the version floor message on one line.** The npm launcher of fallow prints a `verified:` line after the version line. The resolver of the hook read both lines as the version, so the message that blocks a fallow below 3.43.0 broke over two lines. The resolver now reads the version from the first line only, as the commit gate script does.
