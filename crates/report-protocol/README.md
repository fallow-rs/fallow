# fallow-report-protocol

This crate holds the report contract that the fallow command-line tools share.
A tool that emits a fallow report uses these types, so all reports have the
same shape.

The crate contains:

- `ErrorOutput`: the JSON error document that a command writes to stdout when
  it fails with `--format json`.
- `GateOutcome`, `GateStatus`, `GateFile` and `GateFileReason`: the verdict of
  one gate on one run.
- `exit_code`: the process exit codes of the contract.
- `REPORT_PROTOCOL_VERSION`: the version of the wire contract.

The crate version follows the fallow release. The wire version is
`REPORT_PROTOCOL_VERSION`, not the crate version.

Enable the `schema` feature to derive `schemars::JsonSchema` for each type.

The crate depends on no other fallow crate and on no JavaScript type.

## License

MIT
