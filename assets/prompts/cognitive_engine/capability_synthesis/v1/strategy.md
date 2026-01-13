Synthesis strategy:
1) Understand the capability goal from description and intent type.
2) Assume input arrives under `:data` (vector of maps) unless schemas dictate otherwise.
3) For grouping/counting: iterate items, extract labels or keys, build a map label -> {:count N :last_updated "<ts>"}
4) For formatting/output: produce deterministic map/vector results; avoid printing.
5) Always guard against nil / wrong shapes; default to empty map/vector.
6) Keep implementation pure and deterministic.










