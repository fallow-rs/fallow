### Fixed

- **`fallow mutation` deletes the companion temp directories when you stop a run with Ctrl+C.** A signal ends Fallow without its normal cleanup, so the temp directories of the companions stayed on the disk until the next run. The signal handler now deletes them after it ends the companion processes.
