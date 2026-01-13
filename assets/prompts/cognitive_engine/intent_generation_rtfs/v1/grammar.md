# RTFS Intent Form

Form:
```
(intent "name" :goal "..." [:constraints {:k "v" ...}] [:preferences {:k "v" ...}] [:success-criteria "..."])
```
Rules:
- Exactly one top-level `(intent ...)`.
- name: snake_case, descriptive.
- Keys allowed: :goal :constraints :preferences :success-criteria.
- All constraint & preference values are strings.
- Provide :success-criteria if measurable outcome exists.
