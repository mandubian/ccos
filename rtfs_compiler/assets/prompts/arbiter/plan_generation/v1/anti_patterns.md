### ANTI-PATTERN 1: Missing Required Properties
INCORRECT: (plan my-plan :body (do (step "test" (call :ccos.echo "hello"))))
CORRECTED: (plan my-plan :intent-ids ["my-intent"] :body (do (step "test" (call :ccos.echo "hello"))))

### ANTI-PATTERN 2: Mismatched Capabilities
INCORRECT: 
(plan my-plan 
  :intent-ids ["my-intent"]
  :capabilities-required [:ccos.echo]
  :body (do (step "test" (call :database/query "SELECT *"))))
CORRECTED: 
(plan my-plan 
  :intent-ids ["my-intent"]
  :capabilities-required [:ccos.echo :database/query]
  :body (do (step "test" (call :database/query "SELECT *"))))

### ANTI-PATTERN 3: Inconsistent Schemas
INCORRECT: 
(plan my-plan 
  :intent-ids ["my-intent"]
  :input-schema { :user-id :string }
  :output-schema { :result :string }
  :body (do (step "process" (call :process {:user-id 123 :extra-field "data"}))))
CORRECTED: 
(plan my-plan 
  :intent-ids ["my-intent"]
  :input-schema { :user-id :string :extra-field :string }
  :output-schema { :result :string }
  :body (do (step "process" (call :process {:user-id "123" :extra-field "data"}))))

### ANTI-PATTERN 4: Multiple Plan Blocks
INCORRECT: (plan plan1 ...) (plan plan2 ...)
CORRECTED: (plan plan1 ...) // Only one top-level plan

### ANTI-PATTERN 5: Missing Schema Validation
INCORRECT: 
(plan my-plan 
  :intent-ids ["my-intent"]
  :body (do (step "process" (call :process data))))
CORRECTED: 
(plan my-plan 
  :intent-ids ["my-intent"]
  :body (do 
    (step "Validate Input" (call :validate/schema input-schema data))
    (step "Process" (call :process data))
    (step "Validate Output" (call :validate/schema output-schema result))))

### ANTI-PATTERN 6: Unbalanced Parentheses
INCORRECT: (plan my-plan :intent-ids ["my-intent"] :body (do (step "test" (call :ccos.echo "hello"))
CORRECTED: (plan my-plan :intent-ids ["my-intent"] :body (do (step "test" (call :ccos.echo "hello"))))
