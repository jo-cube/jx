# Absolute execution efficiency

Benchmark-only, compile-once/evaluate-many controls. The harness adds no engine dependencies or public API routing. Legacy is the
default; `experimental` remains explicit. Generated inputs, binaries, samples and
profiles belong in a fresh ignored destination under `target/`.

## Reproduce

Run from the repository root with Rust 1.98.1. `prepare.py` builds an independent
Cargo crate so Serde/CPU-clock instrumentation never becomes an engine dependency.
It uses the existing read-only `jkq` adapter script, copies sibling worker sources,
and records source/binary identities. An existing cached dependency registry is
required for the offline build; ordinary tests do not depend on this harness.

```sh
python3 benchmarks/absolute/prepare.py target/absolute-run --jkq ../jkq
python3 benchmarks/absolute/prepare.py target/absolute-run --jkq ../jkq --default-only
cargo test --manifest-path target/absolute-run/harness/Cargo.toml --release --offline
ABS_CHECK=1 target/absolute-run/harness/target/release/jx-absolute
ABS_LIST=1 target/absolute-run/harness/target/release/jx-absolute
python3 benchmarks/absolute/run.py target/absolute-run/harness/target/release/jx-absolute target/absolute-run/full-final
python3 benchmarks/absolute/run.py target/absolute-run/harness/target/release/jx-absolute target/absolute-run/phases --suite phases
python3 benchmarks/absolute/run.py target/absolute-run/harness/target/release/jx-absolute target/absolute-run/counters --suite counters --ms 1500 --samples 3
python3 benchmarks/absolute/run.py target/absolute-run/harness/target/release/jx-absolute target/absolute-run/parallel --suite parallel --ms 1500
python3 benchmarks/absolute/run.py target/absolute-run/harness/target/release/jx-absolute target/absolute-run/profiles --suite profiles
python3 benchmarks/absolute/run.py target/absolute-run/harness/target/release/jx-absolute target/absolute-run/references --suite references
python3 benchmarks/absolute/run.py target/absolute-run/harness/target/release/jx-absolute target/absolute-run/worker-boundary --suite worker-boundary --ms 300 --samples 7
python3 benchmarks/absolute/run.py target/absolute-run/harness/target/release/jx-absolute target/absolute-run/default-controls --suite default --ms 300 --samples 7
python3 benchmarks/absolute/run.py target/absolute-run/harness/target/release/jx-absolute target/absolute-run/simd-comparison --suite simd
python3 benchmarks/absolute/run.py target/absolute-run/harness/target/release/jx-absolute target/absolute-run/simd-counters --suite simd-counters --ms 1000 --samples 3
python3 benchmarks/absolute/run.py target/absolute-run/harness/target/release/validation target/absolute-run/validation --suite validation
```

Use `ABS_CASE` for exact names (`|` joins a subset), `ABS_MODE`, `ABS_PHASE`,
`ABS_MS` and `ABS_SAMPLES` for direct controls. `run.py` records commands and hashes,
runs two reversed independent-process orders, and preserves every observation.
Process ranges are not confidence intervals. Counter/RSS and sampling commands use macOS tools. Builds, checks and sampling must not
run concurrently with throughput timing. Sampled timings are diagnostic only.
Keep the power source and power-saving mode constant. On macOS, record `pmset -g
batt` and `pmset -g custom`; a temporary `caffeinate -i` assertion can prevent idle
sleep during a campaign. Exclude sleep/power-transition samples from absolute-rate
comparisons rather than pooling them with unrestricted runs.

## Work and obligations

`fixtures.rs` defines all expressions and deterministic corpora. `ABS_LIST` emits
actual input/output lengths, numeric field counts, cardinalities, exact source and
worker flags as JSON lines. Names containing a size describe a target; always use
the reported actual bytes. Core projection records have eight root members, a
`metrics` object of the indicated width, and a one-field `profile`. Projection
outputs double each metric and add ID, Unicode uppercase name, immutable label
lookup and region. Widths span 8/24/32/40/80, records 500 B–1 MiB, with direct,
lexical, conditional and function forms.

Scalar and immutable-table controls use 64 records, 500 B–1 MiB. Lookup/predicate
records include 25% table misses. The predicate checks environment, membership of a
looked-up label in three permitted labels, a numeric threshold and region. The
immutable table has 4,096 entries; preparation is excluded from per-record timing.
The name contains a JSON Unicode escape. Core projection corpora have 16 distinct
IDs/flags. Numeric values are deterministic small integers represented as JSON
numbers. These are controlled distributions, not a replay of production traffic.

Sequences span 0/1/2/8/128/1,024/8,192 rows, 0/50/100% positive values and direct,
lexical, callback and output forms. Compact rows are exactly 48 bytes (including
trailing JSON whitespace); padded rows are 500 bytes. They map `v0*v1+v0` and
optionally filter `v0>0`. Empty native sequences are missing, not numeric zero;
callback output preserves singleton versus multiple-result normalization.
Nested paths have depths 2/8/32, two numeric leaves and a two-byte output.

Actual workers use the committed worker's original generator: 64 projection
payloads with one absent payload, drop on non-production, then tombstone on deleted,
then project. Seven drop, four present tombstone, one absent tombstone and 52
projection outcomes occur per cycle. Numeric workers fold 8/1,024 complete 500-byte
records with eight metrics each. Input copying, `Option<Vec<u8>>` ownership transfer,
recycled output capacity (16 KiB cap), payload replacement and result consumption
are included. Kafka, physical I/O, network and worker scheduling are excluded.

## Handwritten references

`handwritten.rs` parses selected fields with Serde seeds, borrows unescaped strings,
discards unused values, and writes JSON directly to the same kind of reused buffer.
It is useful for dynamic projection widths. It is **not** an efficient minimal
numeric-row representation: its 80-slot row is retained as a measured comparison.
`typed.rs` independently uses two-float derived row types, incremental reduction or
an ordinary `Vec` plus Rust loop. It tests whether simple materialization beats
streaming, without implementing any new `jx` execution path.

- `rust`: complete `jx::validate` first, then selective parsing and handwritten work.
- `typed`: the same complete validation, minimal streaming numeric rows; other
  operations use `rust`.
- `typed-vec`: the same validation and numeric computation, with a materialized Vec.
- `serde` / `typed-serde`: parser-only counterparts with weaker error precedence and
  validation policies; report separately.
- `typed-raw`: complete borrowed RawValue preprocessing, then typed/selective work;
  its validation limits differ and it adds an allocation.
- `simd` / `simd-copy` (separate `simd-reference` binary): complete typed tape,
  reused parser buffers and tape. `simd-copy` also copies the worker source to
  preserve its bytes. Core input copies and worker ownership/cap behavior are timed.
  `ABS_POLICIES=1` reports specific validation differences. Parser/tape retained
  capacities are printed separately; zero hot allocations does not mean zero state.
- `legacy` / `foundation`: actual compiled engines, common full validation and
  successful complete JSON output obligations.

**All handwritten modes are restricted to the documented schema.** They assume
unique keys, correctly typed present fields, ordinary scalar numbers and object
array elements. They do not implement arbitrary JSONata cardinality on unexpected
shapes, container identity, effects, lexical mutation, cancellation, coercion or
error categories. Ratios against them measure successful-work overhead on this
verified domain, not efficiency relative to a full-semantics replacement. The
strict references deliberately validate twice (the common validator and selective
parser); no claim of an optimal Rust ceiling is made.

The SIMD tape validates/decodes all input rather than only demanded fields. It
rejects an overflowing number token JX accepts as JSON, and has a different depth
policy. It does not establish general JSONata or raw-token/identity API parity.
This dependency is benchmark-only; it is not a proposed mandatory input DOM/index.

The independent `validation` binary contrasts `jx`, borrowed Serde `RawValue`, and
`IgnoredAny`, and emits explicit malformed/UTF-8/depth policy probes with `ABS_CHECK`.
Do not equate acceptance policies based on successful timing inputs alone.
[Serde's discard visitor](https://serde.rs/ignored-any.html) avoids retained data;
[RawValue](https://docs.rs/serde_json/1.0.150/serde_json/value/struct.RawValue.html)
retains a borrowed raw JSON fragment. Neither is a mandatory DOM.

## Measurement boundaries

`full` validates, acquires, transforms, serializes and consumes reused output.
`prepare` performs validation and static acquisition only. `validated` excludes the
initial validation but still acquires on each iteration. `prepared` reuses one
record's acquisition and includes execution/serialization. `values` reuses prepared
input and delivers values without encoding; its public consumption contract differs
from complete serialization, so subtracting it is a diagnostic, not an additive
CPU accounting identity. `compile` creates and drops programs against an already
prepared immutable binding. Prepared-input bandwidth is logical record bandwidth,
not bytes necessarily read. Rust prepared numeric controls reduce pre-parsed pairs.

Timing disables allocation instrumentation. Separate warmed probes (128 operations, rounded up to a complete worker-trial input cycle)
count alloc/realloc requests, requested bytes and incremental peak requested live
heap. Inputs, compiled code, immutable tables, stack, existing output capacity and
allocator fragmentation are excluded. No live heap may remain after a warmed record.
These metrics are not RSS. `/usr/bin/time -l` provides whole-process RSS, instructions
and cycles on macOS; setup/warmup/assertions remain in those totals. Counter runs
amortize them over seconds and report that limitation. Process CPU clocks give CPU
nanoseconds/record and occupied cores during the actual timing intervals.

Parallel controls run independent single-threaded full transformations with private
compiled/acquisition/output state, synchronized after setup. They measure 1/2/4/8
threads on the host's heterogeneous cores, without affinity. They are in-memory
scaling controls, not Kafka or actual-worker scheduling benchmarks.

## Preserved engine comparisons

`compare.py BEFORE AFTER DESTINATION` runs the same benchmark contract in reversed
independent processes and records hashes, raw samples, process medians, CPU costs,
bandwidth and allocation probes. `--references` includes legacy and the restricted
JX-validated Rust control from BEFORE. `--simd-reference PATH` adds the separately
labelled schema-assuming SIMD control. `--extra-cases` adds selectors; `--phase compile`
should select core expressions, because worker modes always measure complete worker
execution. The recorded CSV phase is authoritative.

The sequence matrix also includes direct, lexical-predicate, function-call and
unused-helper callback formulations. Inputs and successful transformation outputs
match the original numeric reference domain. These controls detect eligibility and
ownership cliffs; they do not expand the references' JSONata semantics.

Keep raw measurements, binary/source hashes and profiles in the ignored run
destination. Investigation-specific analysis scripts and reports are review
artifacts, not dependencies of this suite.

## Opt-in facade controls

`ABS_API=1` compares `legacy`, diagnostic `scan`, automatically acquired `program`,
ordinary `facade`, reusable `workspace`, reusable-template `prepared`, and immutable
`InputPlan` `shared` modes. `focus` explicitly supplies the same validated root as
focus, checking provenance through the settings boundary. Core modes use identical validation and cancellable
serialization. `ABS_PHASE=validated` excludes grammar validation; `compile` measures
compilation separately. `eval/literal`, `eval/dynamic` and `context/128` extend the
existing deterministic cases. Worker API modes are `legacy`, `prepared` and `facade`;
the facade worker changes only compilation selection in copied sibling sources.
The sibling checkout is read-only. Worker completion/ownership obligations match;
the existing prepared adapter may use its complete-output consumer.

```sh
ABS_API=1 ABS_MODE=facade ABS_CASE=project/1024/24/direct target/absolute-run/harness/target/release/jx-absolute
ABS_API=1 ABS_MODE=workspace ABS_CASE=project/1024/24/direct ABS_API_THREADS=4 ABS_MS=1000 target/absolute-run/harness/target/release/jx-absolute
python3 scripts/check-execution-facade.py target/facade-contracts
```

`ABS_API_THREADS` shares one compiled expression across callers with local offsets
and output buffers. It disables allocation instrumentation and reports aggregate
wall/CPU throughput; single-worker probes remain the memory reference. The contract
replay changes only compilation selection, fixture paths, and two native-installation
assertions to require the documented zero-kernel IR policy. Result, ownership, error,
effect, control and consumer assertions remain unchanged.

## Shared-program worker trial

`nested-program/SIZE/WIDTH/{direct,eval,eval-focus}` applies the same production-shaped
projection on sixteen records through direct and immutable nested programs. Validation,
serialization and output obligations match; the suite's schema-only Rust caveats still
apply. These cases check dependency propagation across program boundaries.

`ABS_WORKER_TRIAL=1` runs the copied original worker using only public `Expression`
and `InputPlan` APIs. `ABS_MODE=legacy` or `facade` selects compilation alone. The
`trial/{500/8,1024/40,16384/40}/direct` cases use three ordered drop predicates, two
tombstone predicates, a 4,096-entry immutable lookup table, numeric/string conversions
and projections. `trial/1024/40/eval` wraps the same projection in invariant dynamic
code. Each 64-record cycle has 18 drops, seven tombstones (one absent input), and
39 projections. Sizes are the target present-record sizes; report weighted measured
input/output bandwidth. `trial/{8,128}/{fold,callbacks}` uses alternating present
arrays of 500-byte rows and absent payloads, with scalar reduction or callback output.
These distributions differ from the original worker controls and must be labeled.

```sh
ABS_WORKER_TRIAL=1 ABS_CHECK=1 target/absolute-run/harness/target/release/jx-absolute
ABS_WORKER_TRIAL=1 ABS_MODE=facade ABS_CASE=trial/1024/40/direct target/absolute-run/harness/target/release/jx-absolute
ABS_WORKER_TRIAL=1 ABS_MODE=legacy ABS_THREADS=4 ABS_MS=1000 target/absolute-run/harness/target/release/jx-absolute
```

Before timing, both workers assert identical complete `Execution` outcomes for every
record. Single-worker probes include input copies, shared validation/acquisition,
action selection, serialization and output-capacity recycling. Parallel runs share
one immutable compiled plan, use separate workers/input plans/output buffers, and
report aggregate process CPU and wall time. Allocation instrumentation is disabled
for parallel runs; report single-worker allocation probes separately. Workers are
synchronous and private to the harness. This measures no Kafka broker, pipeline
scheduler, consumer offset handling, network, or physical I/O.


`application/SIZE/REJECT/{direct,eval}` extends the same copied-worker trial with
nested customer/event attributes, compound lookup keys, independent immutable
whitelist and blacklist tables, numeric and string conversions, and tombstones.
Sizes are 500/1,024/16,384 bytes; `REJECT` is the whitelist cutoff 32/128/224 in a
256-record cycle, **not** the final rejection percentage. Other predicates also
reject records. Observed drop/tombstone/projection counts are 46/13/197,
136/8/112 and 227/3/26; tombstones include one absent input. Whitelist sizes are
3,584/2,048/512, blacklist size 133 and label table size 4,096. Cases use 8 or 40
numeric output fields plus five metadata fields. The sole Eval form is 1 KiB,
cutoff 128, with the same projection. These successful-work distributions are
controlled fixtures, not a production-traffic replay.

```sh
ABS_WORKER_TRIAL=1 ABS_MODE=facade ABS_CASE=application/1024/128/direct target/absolute-run/harness/target/release/jx-absolute
ABS_WORKER_TRIAL=1 ABS_MODE=legacy ABS_CASE=application/1024/128/direct ABS_THREADS=4 target/absolute-run/harness/target/release/jx-absolute
ABS_WORKER_TRIAL=1 ABS_CHECK=1 ABS_EXPORT=target/worker-fixtures target/absolute-run/harness/target/release/jx-absolute
```

`ABS_EXPORT` writes reviewed input/expression/binding/outcome fixtures to an ignored
destination. It does not add an engine API. Trial warmup and allocation probes
cover complete input cycles, so a late first projection cannot be mistaken for a
retained-value leak or bias allocations toward an early rejection segment.

For source/feature regression isolation, hold the harness, source path, package
identity, lockfile, compiler flags and input/output contract fixed; rebuild source
snapshots sequentially and preserve each executable before rebuilding. Verify a
repeat baseline build and inspect executable sections as well as whole-file hashes.
A default-only harness and the full experimental harness have different linked
code and cannot alone identify the source of a small coexistence regression.
Retain power checks, independent process orders and raw profiles outside the
intended committed benchmark tree.
