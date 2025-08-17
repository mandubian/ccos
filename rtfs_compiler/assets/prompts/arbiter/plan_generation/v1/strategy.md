# AI GENERATION STRATEGY:
# ================================
# 
# STEP 1: ANALYZE INTENT
# - Extract goal, constraints, preferences, success-criteria
# - Identify required input data from :constraints
# - Determine expected output shape from :success-criteria
# - Note any execution preferences (speed, cost, reliability)
# 
# STEP 2: DESIGN SCHEMAS
# - Input Schema: Derive from Intent :constraints (types, required fields)
# - Output Schema: Derive from Intent :success-criteria (expected result shape)
# - Use RTFS map syntax with type keywords (:string, :int, :float, :bool, :map, :vector)
# 
# STEP 3: DEFINE POLICIES
# - Cost limits: Extract from :constraints or use defaults
# - Retry policy: Based on reliability requirements
# - Timeout: Based on speed preferences
# - Security: Any security constraints from Intent
# 
# STEP 4: IDENTIFY CAPABILITIES
# - List all capabilities the plan will call
# - Include CCOS built-ins (:ccos.echo, :ccos.log, etc.)
# - Add vendor capabilities as needed
# 
# STEP 5: CREATE EXECUTION BODY
# - Use (step "Name" ...) for major milestones
# - Use (call :capability args) for capability invocations
# - Keep steps minimal but sufficient
# - Ensure proper error handling and validation
# 
# STEP 6: ADD ANNOTATIONS
# - Include prompt-id and version for provenance
# - Add generation timestamp
# - Record LLM provider if known
# 
# STEP 7: SYNTAX VALIDATION
# - Ensure single (plan ...) block with balanced parentheses
# - Verify all required properties are present
# - Check that capabilities-required matches body usage
# 
# COMMON PATTERNS:
# - Data validation: (step "Validate Input" (call :validate/schema input-schema data))
# - Error handling: (step "Handle Errors" (call :ccos.log {:level :error :message error}))
# - Success criteria: (step "Validate Output" (call :validate/schema output-schema result))
