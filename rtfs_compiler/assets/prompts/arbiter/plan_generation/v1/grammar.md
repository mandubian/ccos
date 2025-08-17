# RTFS 2.0 Plan Grammar (excerpt)
# A plan is represented as: (plan <name-symbol> :property value ...)
# Required: :intent-ids (vector), :body (RTFS expression)
# Optional: :input-schema {...}, :output-schema {...}, :policies {...}, :capabilities-required [...], :annotations {...}
# 
# Input/Output Schemas: Use RTFS map syntax with type constraints
# Example: { :user-id :string :age :int :email :string }
# 
# Policies: Execution constraints and behavior
# Example: { :max-cost 100.0 :retry {:max 3 :backoff :exponential} :timeout 30000 }
# 
# Capabilities Required: Vector of capability IDs this plan depends on
# Example: [:ccos.echo :vendor/openai :database/query]
# 
# Annotations: Provenance and metadata
# Example: { :prompt-id "plan_generation" :prompt-version "v1" :llm "openai:gpt-4o" :generated-at "2024-01-15T10:30:00Z" }
# 
# Body: The executable RTFS program, typically (do (step "Name" (call :cap args)) ...)
# Use (step "Name" ...) for major milestones, (call :cap ...) for capability invocations
