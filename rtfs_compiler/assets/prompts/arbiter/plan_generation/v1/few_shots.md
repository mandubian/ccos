### Example 1: Simple Data Processing Plan
Intent: (intent process-user-data
  :goal "Process user data and return a summary"
  :constraints { :input-type :map :output-format :string }
  :success-criteria (fn [result] (and (string? result) (not (empty? result)))))

RTFS Plan:
(plan process-user-data-plan
  :intent-ids ["process-user-data"]
  :input-schema { :user-data :map :fields [:name :age :email] }
  :output-schema { :summary :string :word-count :int }
  :policies { :max-cost 50.0 :timeout 30000 }
  :capabilities-required [:ccos.echo :data/process :text/summarize]
  :annotations { :prompt-id "plan_generation" :prompt-version "v1" :generated-at "2024-01-15T10:30:00Z" }
  :body (do
    (step "Validate Input" (call :validate/schema input-schema user-data))
    (step "Process Data" (call :data/process user-data))
    (step "Generate Summary" (call :text/summarize processed-data))
    (step "Validate Output" (call :validate/schema output-schema result))))

### Example 2: Complex Analysis with Error Handling
Intent: (intent analyze-sales-data
  :goal "Analyze Q2 sales data and generate comprehensive report"
  :constraints { :quarter "Q2" :report-format :map }
  :success-criteria (fn [result]
    (and (map? result)
         (contains? result :summary) (string? (get result :summary))
         (contains? result :total_revenue) (float? (get result :total_revenue))
         (> (get result :total_revenue) 50000.0))))

RTFS Plan:
(plan analyze-sales-plan
  :intent-ids ["analyze-sales-data"]
  :input-schema { :sales-data :map :quarter :string :filters :map }
  :output-schema { :summary :string :total_revenue :float :quarter :string :charts :vector }
  :policies { 
    :max-cost 200.0 
    :retry {:max 3 :backoff :exponential}
    :timeout 120000
    :security {:encryption :required}
  }
  :capabilities-required [:database/query :analytics/process :chart/generate :ccos.log]
  :annotations { :prompt-id "plan_generation" :prompt-version "v1" :llm "openai:gpt-4o" }
  :body (do
    (step "Query Sales Data" (call :database/query {:table "sales" :quarter "Q2"}))
    (step "Process Analytics" (call :analytics/process sales-data))
    (step "Generate Charts" (call :chart/generate analytics-result))
    (step "Create Report" (call :report/compile {:summary analytics-summary :charts charts}))
    (step "Validate Revenue" (call :validate/revenue {:min 50000.0} report))
    (step "Log Success" (call :ccos.log {:level :info :message "Sales analysis completed"}))))

### Example 3: Multi-Step Workflow with Conditional Logic
Intent: (intent deploy-application
  :goal "Deploy application to production with safety checks"
  :constraints { :environment :production :safety :required }
  :preferences { :speed :medium :reliability :high }
  :success-criteria (fn [result] (and (map? result) (contains? result :status) (= (get result :status) "deployed"))))

RTFS Plan:
(plan deploy-app-plan
  :intent-ids ["deploy-application"]
  :input-schema { :app-config :map :environment :string :version :string }
  :output-schema { :status :string :url :string :deployment-id :string }
  :policies { 
    :max-cost 500.0 
    :timeout 300000
    :rollback {:enabled true :threshold 0.95}
  }
  :capabilities-required [:infra/validate :deploy/application :monitor/health :rollback/trigger]
  :annotations { :prompt-id "plan_generation" :prompt-version "v1" }
  :body (do
    (step "Validate Configuration" (call :infra/validate app-config))
    (step "Pre-deployment Check" (call :monitor/health {:environment "staging"}))
    (step "Deploy Application" (call :deploy/application {:config app-config :env "production"}))
    (step "Health Check" (call :monitor/health {:environment "production" :timeout 60000}))
    (step.if (< health-score 0.95)
      (step "Rollback" (call :rollback/trigger {:deployment-id deployment-id}))
      (step "Deployment Complete" (call :ccos.log {:level :info :message "Deployment successful"})))))
