// RTFS 2.0 Intent Grammar (compact excerpt)
// (intent <name-symbol>
//   :goal               "..."
//   :original-request   "..."
//   :constraints        { <k> <v> ... }
//   :preferences        { <k> <v> ... }
//   :success-criteria   (fn [result] <boolean-expr>))
// Keys are keywords (:k). Values can be string, number, boolean, keyword, vector, map, or function.
// Keep it single-block, balanced parentheses, no prose outside S-expr.

// Typing patterns (useful examples)
// - Require string:            { :input-type :string }
// - Require integer range:     { :min 0 :max 100 }
// - Allowed enum-like values:  { :mode :fast } ; values often keywords
// - Structured requirement:    { :fields ["name" "age"] }
// - Data source hint:          { :source :sales-db }

// Success criteria (fn [result] ...):
// - Type check:            (string? result)
// - Map checks:            (and (map? result) (contains? result :summary))
// - Numeric bound:         (> (get result :score) 0.85)
// - Vector non-empty:      (and (vector? result) (not (empty? result)))
