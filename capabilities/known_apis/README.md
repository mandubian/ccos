# Known APIs

This directory contains pre-built API definitions for common REST APIs that don't have OpenAPI specifications.

## Structure

Each `.toml` file defines one API with its endpoints, authentication, and other metadata.

## File Format

```toml
[api]
name = "example"
title = "Example API"
base_url = "https://api.example.com"
version = "1.0"
description = "Example API description"

[auth]
type = "api_key"           # api_key, bearer, oauth2, none
location = "query"         # query, header
param_name = "appid"       # Parameter name for the key
env_var = "EXAMPLE_API_KEY"
required = true

[rate_limits]
requests_per_minute = 60
requests_per_day = 1000

[[endpoints]]
id = "get_data"
name = "Get Data"
description = "Retrieve data from the API"
method = "GET"
path = "/v1/data"

[[endpoints.params]]
name = "id"
type = "string"
location = "query"         # query, path, header, body
required = true
description = "Data identifier"

[[endpoints.params]]
name = "format"
type = "string"
location = "query"
required = false
description = "Response format"
```

## Adding a New API

1. Create a new `.toml` file named after the API domain (e.g., `openweathermap.toml`)
2. Define the API metadata, authentication, and endpoints
3. Test with: `ccos server search "api name"`

## Supported APIs

- `openweathermap.toml` - OpenWeatherMap API
- (more to be added)
