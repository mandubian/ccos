### Few-shot examples (compact)
(intent greet-user
  :goal "Generate a personalized greeting"
  :original-request "Greet a user by name"
  :constraints { :name-type :string }
  :preferences { :tone :friendly }
  :success-criteria (fn [result]
    (and (string? result)
         (str/includes? result "Hello")
         (not (empty? result)))))

(intent validate-email
  :goal "Validate email format"
  :original-request "Check user's email"
  :constraints { :input-type :string }
  :success-criteria (fn [result]
    (and (bool? result))))

(intent analyze-q2-sales
  :goal "Analyze Q2 sales data and produce KPIs"
  :original-request "Analyze Q2 sales data"
  :constraints { :quarter "Q2" :data-source :sales-db :report-format :map }
  :preferences { :include-charts false }
  :success-criteria (fn [result]
    (and (map? result)
         (contains? result :total_revenue)
         (float? (get result :total_revenue))
         (> (get result :total_revenue) 50000.0))))
