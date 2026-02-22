---
name: Nitter Feed
description: Fetches recent tweets from an X (Twitter) user using a Nitter RSS feed, avoiding the bloated X website.
version: "1.0.0"
capabilities:
  - nitter.feed.fetch
data_classifications:
  - Public
approval:
  required: false
  mode: Once
---

# Nitter RSS Feed Fetcher

Use this skill to fetch recent tweets from an X (Twitter) account timeline. Since X heavily restricts direct access, this uses a Nitter instance's RSS feed to retrieve the timeline as XML.

If the default instance `nitter.net` is rate-limited or unavailable, you can substitute it with another known public Nitter instance (e.g., `nitter.poast.org`, `nitter.cz`, `xcancel.com`).

The output will be raw XML (RSS format) containing the latest tweets. You are an expert at parsing XML; extract the tweet content, timestamps, and authors to present a clean summary to the user.

## Fetch Timeline

Fetches the recent timeline tweets as an RSS XML feed for a given X account. Replace `<handle>` with the target Twitter username (e.g., `elonmusk`).

> **Important**: Nitter instances require a **browser-like User-Agent** header. Requests without it are likely to be blocked or return empty responses. Always set `User-Agent` to a realistic browser string (see examples below).

**Important**: Do NOT rely on a single instance. Nitter instances go down frequently. Always try each instance in order and move on immediately on any error (connection error, DNS failure, HTTP 4xx/5xx). Use a short per-request timeout (5 seconds) so failures are detected fast.

Instances to try in order:
1. `https://nitter.net/<handle>/rss`
2. `https://nitter.poast.org/<handle>/rss`
3. `https://nitter.cz/<handle>/rss`
4. `https://xcancel.com/<handle>/rss`

### Shell (curl)

Default approach when not running inside Python:

```bash
UA="Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36"
for base in "https://nitter.net" "https://nitter.poast.org" "https://nitter.cz" "https://xcancel.com"; do
    result=$(curl -sL --max-time 5 -A "$UA" "$base/<handle>/rss")
    if [ -n "$result" ]; then
        echo "$result"
        break
    fi
done
```

### Python

Use when executing inside a Python sandbox:

```python
import requests

MIRRORS = [
    "https://nitter.net",
    "https://nitter.poast.org",
    "https://nitter.cz",
    "https://xcancel.com",
]

HEADERS = {
    "User-Agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36"
}

def fetch_rss(handle):
    last_error = None
    for base in MIRRORS:
        try:
            resp = requests.get(f"{base}/{handle}/rss", headers=HEADERS, timeout=5)
            resp.raise_for_status()
            return resp.text
        except Exception as e:
            last_error = e
            continue
    raise RuntimeError(f"All Nitter mirrors failed. Last error: {last_error}")
```
