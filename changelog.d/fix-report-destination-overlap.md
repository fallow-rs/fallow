### Fixed

- **Fallow now refuses two save or report flags that name one file.** A run with `--output-file out.sarif --sarif-file out.sarif`, or with another spelling of one path such as `./sub/../out.sarif`, wrote both reports to the file, and the later write replaced the earlier report with no error. The run now exits with code 2 before the analysis and names both flags. The check covers each file that a save or report flag writes. A device or a named pipe, such as `/dev/null` or `/dev/stdout`, is still allowed for more than one flag.
