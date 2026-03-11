use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub artifact_type: String,
    pub name: String,
    pub content: String,
}

pub fn extract_artifacts_from_text(text: &str) -> Vec<Artifact> {
    let mut artifacts = Vec::new();
    let re = match regex::Regex::new(
        r#"(?s)<artifact\s+type="([^"]+)"\s+name="([^"]+)">\s*(.*?)\s*</artifact>"#,
    ) {
        Ok(re) => re,
        Err(_) => return Vec::new(),
    };
    for cap in re.captures_iter(text) {
        artifacts.push(Artifact {
            artifact_type: cap[1].to_string(),
            name: cap[2].to_string(),
            content: cap[3].to_string().trim().to_string(),
        });
    }
    artifacts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_artifacts() {
        let text = r#"
Here is some analysis.
<artifact type="code" name="weather_agent.py">
def get_weather():
    pass
</artifact>
And some more text.
<artifact type="design" name="architecture.md">
# Architecture
- Component A
</artifact>
"#;
        let artifacts = extract_artifacts_from_text(text);
        assert_eq!(artifacts.len(), 2);
        assert_eq!(artifacts[0].artifact_type, "code");
        assert_eq!(artifacts[0].name, "weather_agent.py");
        assert_eq!(artifacts[0].content, "def get_weather():\n    pass");
        assert_eq!(artifacts[1].artifact_type, "design");
        assert_eq!(artifacts[1].name, "architecture.md");
    }
}
