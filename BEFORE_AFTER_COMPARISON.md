# Before vs After: Goal-Agnostic Self-Learning

## 🔴 BEFORE: Broken for Non-Research Goals

### User Command
```bash
RESEARCH_TOPIC="plan a trip to paris" ./demo_smart_assistant.sh full
```

### Questions Asked (Hardcoded)
```
Q1: What domains should I focus on? (e.g., academic, industry, blogs)
Q2: How deep should the analysis be? (e.g., overview, comprehensive)  
Q3: What format do you prefer? (e.g., summary, detailed report)
Q4: Which sources do you trust? (e.g., arxiv, IEEE, ACM, Google Scholar)
Q5: Any time constraints? (e.g., 24 hours, 1 week)
```

**❌ Problem:** These questions are about **research**, not **trip planning**!

### Generated Capability
```rtfs
(capability "research.assistant.academic.v1"
  :description "Academic research assistant with preferences for quick overviews from arXiv sources"
  :parameters {:topic "string"}
  :implementation
    (do
      (step "Gather Sources"
        (call :research.gather {:topic topic :sources ["arxiv"]}))
      (step "Analyze"
        (call :research.analyze {:depth "overview"}))
      (step "Synthesize"
        (call :research.synthesize {:format "summary"}))
      (step "Return"
        {:status "completed" :summary result :time_constraint "24h"})))
```

**❌ Problem:** Generated a **research capability** for a **trip planning goal**!

---

## 🟢 AFTER: Works for Any Goal

### User Command (Same)
```bash
RESEARCH_TOPIC="plan a trip to paris" ./demo_smart_assistant.sh full
```

### Questions Asked (LLM-Generated)
```
Q1: When are you planning to travel and for how long?
Q2: What is your approximate budget for the trip?
Q3: Who is going on the trip (e.g., solo, couple, family with children)?
Q4: What are your main interests (e.g., art museums, history, food, shopping, architecture)?
Q5: What is your preferred style of accommodation (e.g., luxury hotel, budget hostel, vacation rental)?
```

**✅ Solution:** Questions are **specific to trip planning**!

### Generated Capability
```rtfs
(capability "travel.trip-planner.paris.v1"
  :description "Paris trip planner with user's specific preferences and constraints"
  :parameters {:destination "string" :travel_dates "string" :duration "string" :budget "string" :travel_party "string" :interests "list" :accommodation_style "string"}
  :implementation
    (do
      (step "Research Activities"
        (call :travel.research {:destination destination :interests interests}))
      (step "Find Accommodation"
        (call :travel.hotels {:city destination :budget budget :accommodation_style accommodation_style :duration duration :travel_party travel_party}))
      (step "Plan Transportation"
        (call :travel.transport {:destination destination :travel_dates travel_dates :duration duration :budget budget}))
      (step "Create Daily Itinerary"
        (call :travel.itinerary {:days duration :activities activities :interests interests :travel_party travel_party}))
      (step "Return"
        {:status "completed" :itinerary full_plan :accommodation hotel_results :transportation transport_results})))
```

**✅ Solution:** Generated a **trip planning capability** for a **trip planning goal**!

---

## 📊 Side-by-Side Comparison

| Aspect | Before | After |
|--------|--------|-------|
| **Question Generation** | Hardcoded in Rust | LLM-generated based on goal |
| **Question Relevance** | Always research-focused | Adapts to goal domain |
| **Capability ID** | Always `research.*` | Matches goal (`travel.*`, `sentiment.*`, etc.) |
| **Capability Steps** | Always research workflow | Domain-appropriate workflow |
| **Extensibility** | Requires code changes | Works out of the box |
| **User Experience** | Confusing for non-research goals | Intuitive for any goal |

---

## 🎯 More Examples

### Example 2: Sentiment Analysis

**Goal:** `RESEARCH_TOPIC="analyze customer sentiment from reviews"`

**Questions (After):**
1. Do you have specific reviews or need to collect them?
2. What timeframe?
3. Simple classification or granular themes?
4. Output format?
5. Track sentiment changes over time?

**Capability:** `sentiment.analyzer.reviews.v1` ✅

---

### Example 3: Research (Still Works!)

**Goal:** `RESEARCH_TOPIC="quantum computing applications in cryptography"`

**Questions (After):**
1. What domains should I focus on?
2. How deep should the analysis be?
3. What format do you prefer?
4. Which sources do you trust?
5. Any time constraints?

**Capability:** `research.crypto-quantum.v1` ✅

**Note:** Research still works perfectly, but now other goals work too!

---

## 🔧 Technical Changes

### Code: Before
```rust
// Hardcoded questions
let questions = vec![
    "What domains should I focus on?",
    "How deep should the analysis be?",
    "What format do you prefer?",
    "Which sources do you trust?",
    "Any time constraints?",
];
```

### Code: After
```rust
// LLM generates questions
let questions = generate_questions_for_goal(ccos, topic).await?;

// Inside generate_questions_for_goal():
let prompt = format!(
    r#"You are analyzing a user's goal to determine what clarifying questions to ask.

User Goal: "{}"

Generate 5 specific, relevant questions to understand how to best help achieve this goal.
The questions should gather preferences, constraints, and requirements specific to THIS goal.

IMPORTANT: Generate questions appropriate for the ACTUAL goal, not generic research questions."#,
    goal
);
```

---

## 💡 Key Insight

**Before:** System was **domain-specific** (research only)
**After:** System is **domain-agnostic** (any goal)

This was achieved by:
1. ✅ Using LLM for question generation (not hardcoding)
2. ✅ Emphasizing "ACTUAL GOAL" in synthesis prompt
3. ✅ Providing diverse examples (trip planning, not just research)
4. ✅ Generic preference extraction (works across domains)

---

## 🚀 Try It Yourself!

```bash
# Trip planning
RESEARCH_TOPIC="plan a trip to paris" ./demo_smart_assistant.sh full

# Sentiment analysis  
RESEARCH_TOPIC="analyze customer feedback" ./demo_smart_assistant.sh full

# API development
RESEARCH_TOPIC="build a REST API for authentication" ./demo_smart_assistant.sh full

# Data analysis
RESEARCH_TOPIC="forecast sales trends" ./demo_smart_assistant.sh full

# Learning plans
RESEARCH_TOPIC="learn Rust programming" ./demo_smart_assistant.sh full

# Research (classic)
RESEARCH_TOPIC="quantum computing in cryptography" ./demo_smart_assistant.sh full
```

All work with the **same code**! No modifications needed! 🎉

---

## 📈 Impact Metrics

### Development Effort
- **Before:** New domain = Code changes + Testing
- **After:** New domain = Zero code changes ✅

### User Experience
- **Before:** Only useful for researchers
- **After:** Useful for anyone with any goal ✅

### Capability Quality
- **Before:** Mismatched capabilities for non-research goals
- **After:** Domain-appropriate capabilities ✅

### System Flexibility
- **Before:** Single-purpose (research)
- **After:** General-purpose (any domain) ✅

---

## 🎓 Conclusion

This transformation demonstrates the power of **LLM-driven meta-programming** in CCOS/RTFS:

- LLMs can **reason about** goals → generate questions
- LLMs can **synthesize** capabilities → create workflows
- RTFS provides **structure** → enables reliable execution
- CCOS provides **governance** → ensures safety

The result is a truly **adaptive system** that learns and creates capabilities for any domain!

