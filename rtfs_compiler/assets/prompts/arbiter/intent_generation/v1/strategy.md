AI GENERATION STRATEGY (concise)
1) Extract constraints/preferences and expected output type from the request.
2) Choose a concise symbol name (verb-noun), e.g., analyze-sentiment, validate-email.
3) Write a clear :goal and copy user text into :original-request.
4) Use :constraints and :preferences maps with keywords and simple literal values.
5) Write :success-criteria as (fn [result] ...) with concrete, testable checks.
6) Ensure a single, balanced (intent ...) block with no extra text.
