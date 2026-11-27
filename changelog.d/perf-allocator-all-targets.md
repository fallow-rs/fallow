### Performance

- **The fallow binaries use the mimalloc allocator on every platform.** Before, only the Linux musl binaries used it. The parallel analysis makes many small allocations, and mimalloc handles them with less CPU time than the system allocators. On macOS, a cold `dead-code` run with all threads used 9% to 29% less CPU time on four public projects. Peak memory rises by a fixed amount of about 12 to 25 MiB on a multi-core machine. On the largest project (about 700 MiB peak), peak memory rose by 5% to 7%.
