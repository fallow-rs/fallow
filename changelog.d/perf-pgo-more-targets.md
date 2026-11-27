### Performance

- **The Linux x64 musl, Linux arm64 glibc, macOS x64 and Windows x64 binaries now use profile-guided optimization.** On the held-out benchmark projects, these binaries need less CPU time than before: Linux x64 musl 12.4% less, Linux arm64 glibc 12.6% less, macOS x64 8.5% less, and Windows x64 about 4.5% less. The Linux x64 glibc and macOS arm64 binaries already used it.
