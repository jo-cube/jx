# jx-native

Internal bounded Cranelift backend for the `jx` JSONata engine. Enable through the
engine/CLI `jit` Cargo feature; this crate is not a general JSONata execution API.
Rust 1.98.1; MIT licensed.

Supported targets: x86_64 Linux/macOS/Windows and aarch64 Linux/macOS. Other targets
fail with a compile-time diagnostic; the default engine remains independent of this
crate. Compiled code is process-local and uses the host ISA, not a portable code cache.

Kernels accept bounded primitive buffers and return guarded results. Validation,
traversal, dynamic calls/effects, allocation and serialization remain in Rust.
Executable code is shared immutably with explicit module ownership. Unsafe invocation
and release are isolated in `src/executable.rs`. The engine falls back on compilation
failure or runtime guards and never publishes a partial native result.
