# Twitter Publisher Skill

A comprehensive sample skill demonstrating the full onboarding blueprint injection system.

## Metadata

- **ID**: `twitter-publisher`
- **Name**: Twitter Publisher

## Capabilities

This skill uses the following CCOS capabilities:
- `ccos.network.http-fetch` - For API calls to Twitter
- `ccos.secrets.set` - For storing API credentials
- `ccos.memory.store` - For persisting configuration

## Operations

### publish-tweet

Publish a tweet to your Twitter account.

- **Endpoint**: `https://api.twitter.com/2/tweets`
- **Method**: POST
- **Runtime**: http
- **Input Schema**:
  ```yaml
  type: object
  properties:
    text:
      type: string
      description: Tweet content (max 280 characters)
      maxLength: 280
  required: [text]
  ```
- **Output Schema**:
  ```yaml
  type: object
  properties:
    id:
      type: string
      description: Tweet ID
    text:
      type: string
      description: Tweet content
  ```

### get-user-info

Get authenticated user information.

- **Endpoint**: `https://api.twitter.com/2/users/me`
- **Method**: GET
- **Runtime**: http
- **Output Schema**:
  ```yaml
  type: object
  properties:
    id:
      type: string
    username:
      type: string
    name:
      type: string
  ```

## Onboarding

This skill requires mandatory onboarding to set up Twitter API credentials and verify account ownership.

### Required: true

### Steps

#### 1. setup-api-key

**Type**: `api_call`

Set up Twitter API bearer token for authentication.

**Operation**: `ccos.secrets.set`

**Params**:
```yaml
key: TWITTER_BEARER_TOKEN
scope: skill
skill_id: twitter-publisher
description: Twitter API Bearer Token for authentication
value: "{{user_input}}"
```

**Store**:
- None (handled by secrets capability)

**Depends On**: []

**Verify on Success**:
```lisp
(audit.succeeded? "ccos.secrets.set")
```

---

#### 2. verify-credentials

**Type**: `api_call`

Verify that the API credentials work by fetching user info.

**Operation**: `twitter-publisher.get-user-info`

**Params**: {}

**Store**:
```yaml
- from: "username"
  to: "memory:twitter_publisher.username"
- from: "id"
  to: "memory:twitter_publisher.user_id"
```

**Depends On**: [`setup-api-key`]

**Verify on Success**:
```lisp
(and
  (audit.succeeded? "twitter-publisher.get-user-info")
  (audit.metadata? "twitter-publisher.get-user-info" "username" "{{memory:twitter_publisher.username}}"))
```

---

#### 3. verify-ownership

**Type**: `human_action`

Verify account ownership by posting a verification tweet.

**Action**:
```yaml
action_type: tweet_verification
title: Verify Twitter Account Ownership
instructions: |
  To verify that you own this Twitter account, please post the following tweet:
  
  > I'm setting up @twitter-publisher skill on CCOS! 🤖 #CCOS #Automation
  
  After posting, paste the tweet URL below (e.g., https://twitter.com/username/status/123456789).
required_response:
  type: object
  properties:
    tweet_url:
      type: string
      pattern: "^https://twitter\\.com/.+/status/\\d+$"
  required: [tweet_url]
```

**Store**:
```yaml
- from: "tweet_url"
  to: "memory:twitter_publisher.verification_tweet_url"
```

**Depends On**: [`verify-credentials`]

**Verify on Success**:
```lisp
(audit.succeeded? "ccos.approval.complete")
```

---

#### 4. mark-operational

**Type**: `api_call`

Mark the skill as fully operational.

**Operation**: `ccos.skill.onboarding.mark_operational`

**Params**:
```yaml
skill_id: twitter-publisher
```

**Depends On**: [`setup-api-key`, `verify-credentials`, `verify-ownership`]

**Verify on Success**:
```lisp
(audit.succeeded? "ccos.skill.onboarding.mark_operational")
```

---

## Instructions

Use this skill to publish tweets and manage your Twitter presence. The skill must complete onboarding before it can be used:

1. **API Key Setup**: Provide your Twitter API bearer token
2. **Credential Verification**: Automatically verify the token works
3. **Ownership Verification**: Post a verification tweet to prove account ownership
4. **Operational**: Skill is ready to publish tweets

### Example Usage

After onboarding, you can use the skill like this:

```lisp
(call :twitter-publisher.publish-tweet {
  :text "Hello from CCOS! 🚀"
})
```

## Secrets

- `TWITTER_BEARER_TOKEN` (skill-scoped) - Twitter API bearer token for authentication

## Examples

### Example 1: Simple Tweet

**Goal**: "Post a tweet saying 'Hello World'"

**Generated Plan**:
```lisp
(call :twitter-publisher.publish-tweet {
  :text "Hello World"
})
```

### Example 2: Tweet with Hashtags

**Goal**: "Post a tweet about AI with hashtags"

**Generated Plan**:
```lisp
(call :twitter-publisher.publish-tweet {
  :text "Exploring the future of AI agents! 🤖 #AI #Automation #CCOS"
})
```

## Architecture Notes

This skill demonstrates the **Skill Onboarding as Data** architecture (Priority 5):

- **Declarative Steps**: Onboarding is defined as structured data, not code
- **Dependency Graph**: Steps declare dependencies explicitly
- **Verification Predicates**: Each step has declarative success criteria using the RTFS audit system
- **Blueprint Injection**: When non-operational, the `DelegatingCognitiveEngine` injects mandatory setup instructions
- **State Tracking**: `SkillOnboardingState` in `WorkingMemory` tracks progress
- **Human-in-the-Loop**: Seamlessly integrates human verification steps via approval system

This skill is used for integration testing and as a reference implementation.
