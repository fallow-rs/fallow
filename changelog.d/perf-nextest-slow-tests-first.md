### Changed

- **Pull request CI finishes sooner.** Nextest starts tests in binary and name order, so a slow test that sorts late ran alone at the end of a CI test partition, and the partition waited for it. The `ci` nextest profile now starts the tests that take 25 seconds or more first. This change affects contributors only.
