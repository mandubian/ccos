# Goal-Agnostic Demo Examples

The self-learning demo now adapts to **any goal domain**. Here are real examples tested:

## 🧳 Travel Planning

```bash
RESEARCH_TOPIC="plan a trip to paris" ./demo_smart_assistant.sh learn
```

**Generated Questions:**
1. When are you planning to travel and for how long?
2. What is your approximate budget for the trip?
3. Who is going on the trip (solo, couple, family)?
4. What are your main interests (museums, food, shopping)?
5. What is your preferred accommodation style?

**Generated Capability:**
```rtfs
(capability "travel.trip-planner.paris.v1"
  :description "Paris trip planner with user's specific preferences"
  :parameters {:destination :budget :interests :accommodation_style}
  :implementation
    (do
      (step "Research Activities"
        (call :travel.research {:destination destination :interests interests}))
      (step "Find Accommodation"
        (call :travel.hotels {:budget budget :style accommodation_style}))
      (step "Plan Transportation"
        (call :travel.transport {:destination destination}))
      (step "Create Itinerary"
        (call :travel.itinerary {:days duration :activities attractions}))
      (step "Return"
        {:status "completed" :itinerary full_plan})))
```

---

## 📊 Sentiment Analysis

```bash
RESEARCH_TOPIC="analyze customer sentiment from reviews" ./demo_smart_assistant.sh learn
```

**Generated Questions:**
1. Do you have specific reviews or need to collect them?
2. What timeframe do you want to analyze?
3. Simple classification or granular themes?
4. What output format (report, dashboard, spreadsheet)?
5. Track sentiment changes over time?

**Generated Capability:**
```rtfs
(capability "sentiment.analyzer.reviews.v1"
  :description "Analyze customer sentiment from reviews"
  :parameters {:data_source :timeframe :granularity :output_format}
  :implementation
    (do
      (step "Collect Reviews"
        (call :data.collect {:source data_source :timeframe timeframe}))
      (step "Classify Sentiment"
        (call :nlp.sentiment {:text reviews :granularity granularity}))
      (step "Extract Themes"
        (call :nlp.themes {:text reviews :categories themes}))
      (step "Generate Report"
        (call :reporting.create {:format output_format :data analysis}))
      (step "Return"
        {:status "completed" :analysis sentiment_report})))
```

---

## 🔬 Research (Default)

```bash
RESEARCH_TOPIC="quantum computing applications in cryptography" ./demo_smart_assistant.sh learn
```

**Generated Questions:**
1. What domains should I focus on (academic, industry)?
2. How deep should the analysis be?
3. What format do you prefer?
4. Which sources do you trust?
5. Any time constraints?

**Generated Capability:**
```rtfs
(capability "research.crypto-quantum.v1"
  :description "Research quantum computing in cryptography"
  :parameters {:topic :depth :sources}
  :implementation
    (do
      (step "Gather Sources"
        (call :research.gather {:topic topic :sources sources}))
      (step "Analyze Depth"
        (call :research.analyze {:depth depth}))
      (step "Synthesize Content"
        (call :research.synthesize {:format format}))
      (step "Return"
        {:status "completed" :report result})))
```

---

## 🛠️ Development Tasks

```bash
RESEARCH_TOPIC="build a REST API for user authentication" ./demo_smart_assistant.sh learn
```

**Expected Questions:**
- What programming language/framework?
- What database for user storage?
- Which authentication method (JWT, OAuth, session)?
- What endpoints are needed?
- Any specific security requirements?

**Expected Capability ID:** `development.api-auth.v1`

---

## 📈 Data Analysis

```bash
RESEARCH_TOPIC="create sales forecast from historical data" ./demo_smart_assistant.sh learn
```

**Expected Questions:**
- What time period of historical data?
- Which forecasting model (linear, ARIMA, ML)?
- What granularity (daily, weekly, monthly)?
- Any seasonal patterns to consider?
- What confidence interval?

**Expected Capability ID:** `analytics.sales-forecast.v1`

---

## 🎓 Learning Plan

```bash
RESEARCH_TOPIC="learn Rust programming" ./demo_smart_assistant.sh learn
```

**Expected Questions:**
- What's your programming experience level?
- What's your learning goal (projects, job, hobby)?
- How much time per week?
- Prefer books, videos, or hands-on coding?
- Any specific Rust areas (systems, web, embedded)?

**Expected Capability ID:** `education.rust-learning.v1`

---

## Key Observations

### ✅ Domain Detection
The LLM correctly identifies:
- **travel** → trip planning questions
- **sentiment/analyze** → data analysis questions
- **research** → academic research questions
- **build/create** → development questions

### ✅ Question Relevance
Questions are specific and contextual:
- Travel: budget, dates, interests
- Analysis: data source, timeframe, granularity
- Research: sources, depth, format
- Development: language, architecture, requirements

### ✅ Capability Structure
Generated capabilities follow RTFS patterns:
- Appropriate ID naming (`domain.subdomain.variant.v1`)
- Relevant parameters based on conversation
- Step orchestration matching the workflow
- Domain-appropriate sub-capability calls

### ✅ True Generalization
The same demo code handles **completely different domains** with zero hardcoding!

---

## How It Works

1. **Goal Analysis**: LLM analyzes the user's goal text to determine domain
2. **Question Generation**: LLM generates 5 relevant clarification questions
3. **Conversation Capture**: CCOS records user responses via `ccos.user.ask`
4. **Capability Synthesis**: LLM synthesizes RTFS capability matching the goal and preferences
5. **Registration**: Capability stored in marketplace for reuse

The entire process is driven by two LLM calls:
- `generate_questions_for_goal(goal)` → questions array
- `synthesize_capability_via_llm(goal, conversation)` → RTFS capability

## Try Your Own!

```bash
# Any goal works!
RESEARCH_TOPIC="your creative goal here" ./demo_smart_assistant.sh full
```

The demo will adapt and create a workflow specific to your needs!

