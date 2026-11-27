### Performance

- **The Linux x64 musl, Linux arm64 glibc, macOS x64 and Windows x64 binaries now use profile-guided optimization.** On the held-out benchmark projects, these binaries need less CPU time than the same builds without it: Linux x64 musl 12.4% less, Linux arm64 glibc 14.2% less, macOS x64 10.2% less, and Windows x64 4.1% less. The Linux x64 glibc and macOS arm64 binaries already used it.
