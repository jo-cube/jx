# Callback regression checks

Retained binaries from M27, M28 and M29 were timed sequentially with seven 300 ms
samples per fixture, in both orders. Same compiler/toolchain and allocation counters;
no builds or profiles ran concurrently. M29 here is the unchanged committed engine.

- M28 → M29 partial callback: -0.3% / +0.75%, rather than M29's earlier ~-3%.
- M27 → M29 short string callback: +1.82% / -1.02%, rather than the earlier ~-4%.
- M27 → M29 500 B string callback: -1.32% / +0.07%.

There is no additional executed M29 frame/value/plan operation on the numeric lambda
partial fixture: M29's Math partial coercion branch does not run. M28 adds an optional
arena control pointer and call/plan checks; allocations stay unchanged, with eight
additional requested bytes. These are real structural differences, but do not establish
the cause of the earlier timing delta. Fresh profiles expose no new traversal or
allocation count. Build layout/inlining and measurement variation remain possible;
we cannot attribute historical deltas to exact instruction cycles.

Temporary ablations:

1. Disable the optional call guard and callback plan-control check, retaining the
   existing stack ceilings. String callback -5.15% (100 B) / -1.90% (500 B), partial
   -0.02%, wide string map +0.09%. No evidence these nil guards dominate; restored.
2. Move builtin/matcher checkpoints into their match arms to avoid the preliminary
   kind test for lambdas. String callback +2.5% / -0.82% in opposite orders; numeric
   partial -1.16% / -0.37%, wide string map +1.40% / +0.56%. Removed: marginal or
   inconsistent and no allocation reduction.

These are negative experiments, not performance changes. Their build logs/results
remain; the final evaluator keeps every resource check and original dispatch.
