### Fixed

- **The baseline growth gate ignores scope changes in dead-code baselines.** A workspace save now compares only the finding fields. A new occurrence of a finding still fails the gate.
- **An unchanged health baseline can move from legacy finding keys to counts.** During the first comparison, the gate checks how many distinct findings each new bucket requires. A function can exceed both the complexity and CRAP thresholds. The gate reports the limits of the comparison because legacy keys omit category and severity.
