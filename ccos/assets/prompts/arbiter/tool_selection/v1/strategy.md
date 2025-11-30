Selection strategy:
- Align action verbs (list, create, delete, filter) between need and tool.
- Match domain tokens (github, issues, weather, calendar, finance).
- Compare required inputs to tool input keys; prefer tools that already accept most keys.
- When aliases differ (e.g., :username vs :user_id), propose an input remap.
- Reject tools that have incompatible verbs (create vs list) or wrong domain.
- If two tools are equally good, choose the more specific one (narrower scope).
- Return `nil` when no tool satisfies the need rather than guessing.



