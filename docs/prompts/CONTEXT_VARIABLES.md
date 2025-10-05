# Context Variables in Plan Generation

## Overview

Context variables allow plans to reference data from previous plan executions, enabling more modular and reusable plans while maintaining the current CCOS architecture.

## How It Works

### 1. **Context Passing**
- Results from previous plan executions are passed as `HashMap<String, String>` to the LLM provider
- The LLM receives these as "Available context from previous executions"
- Variables can be referenced using `<context_variable_name>` syntax

### 2. **Variable Syntax**
```lisp
; Reference context variables using angle brackets
(call :ccos.echo {:message (str "Planning activities for your " <trip/duration> "-day trip to " <trip/destination>)})
```

### 3. **Mixing Context with New Data**
```lisp
(let [activity_preferences (call :ccos.user.ask "What activities interest you?")
      special_requests (call :ccos.user.ask "Any special requests?")]
  (call :ccos.echo {:message (str "Planning activities for your " <trip/duration> "-day trip to " <trip/destination>)})
  {:itinerary/activities activity_preferences
   :itinerary/requests special_requests
   :trip/destination <trip/destination>
   :trip/duration <trip/duration>})
```

## Implementation Details

### LLM Provider Changes
- `generate_plan` method now uses the `context` parameter
- Context variables are added to prompt variables with `context_` prefix
- User message includes available context variables

### Prompt Updates
- Grammar includes context variable syntax
- Examples show correct usage patterns
- Task description explains when to use context vs collect new data

## Usage Patterns

### ✅ **Correct Usage**
```lisp
; Use context variables when available
(call :ccos.echo {:message (str "Creating itinerary for your " <trip/duration> "-day trip to " <trip/destination>)})

; Mix context variables with new data collection
(let [new_preference (call :ccos.user.ask "What's your preference?")]
  {:new/preference new_preference
   :trip/destination <trip/destination>})
```

### ❌ **Incorrect Usage**
```lisp
; Don't reference undefined variables
(call :ccos.echo {:message (str "Planning your " duration "-day trip")})  ; ERROR: duration not defined

; Don't assume context variables exist
(call :ccos.echo {:message (str "Trip to " <trip/destination>)});  ; May fail if context not provided
```

## Benefits

1. **Modular Plans**: Plans can be focused on specific tasks
2. **Data Reuse**: Avoid re-collecting information from users
3. **Better UX**: More natural conversation flow
4. **Backward Compatibility**: Works with existing single-plan approach

## Example Flow

1. **First Plan**: "Plan a trip to Paris"
   - Collects: destination, duration, budget, dates
   - Returns: `{:trip/destination "Paris", :trip/duration "5 days", ...}`

2. **Second Plan**: "Create detailed itinerary"
   - Uses context: `<trip/destination>`, `<trip/duration>`, `<trip/budget>`
   - Collects new: activity preferences, special requests
   - Returns: `{:itinerary/activities "...", :trip/destination "Paris", ...}`

3. **Third Plan**: "Add cultural activities"
   - Uses context: `<trip/destination>`, `<trip/duration>`, `<itinerary/activities>`
   - Collects new: cultural preferences, museum priorities
   - Returns: `{:cultural/museums "...", :trip/destination "Paris", ...}`

## Implementation Status

- ✅ LLM provider updated to pass context
- ✅ Prompts updated with context variable syntax
- ✅ Examples added showing correct usage
- ✅ Documentation created
- 🔄 Example implementation created (demonstration only)

## Next Steps

To fully implement context passing in the main example:

1. **Modify `user_interaction_progressive_graph.rs`** to extract results from successful plan executions
2. **Pass results as context** to subsequent plan generations
3. **Update the conversation flow** to accumulate context across turns
4. **Test with real LLM interactions** to ensure context variables are properly used

This approach provides a clean way to enable modular plans while maintaining the current CCOS architecture.
