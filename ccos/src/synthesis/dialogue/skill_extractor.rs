//! Advanced Skill Extraction
//!
//! Implements Phase 2: Skill extraction and automatic parameter mapping.
//! Provides sophisticated analysis of interaction patterns to derive agent skills.

use crate::synthesis::InteractionTurn;
use std::collections::{HashMap, HashSet};

/// Extracted skill with confidence score and supporting evidence
#[derive(Debug, Clone)]
pub struct ExtractedSkill {
    pub skill: String,
    pub confidence: f64,
    pub evidence: Vec<String>,
    pub category: SkillCategory,
}

/// Categories of skills that can be extracted
#[derive(Debug, Clone, PartialEq)]
pub enum SkillCategory {
    DomainSpecific,
    Orchestration,
    Analysis,
    Communication,
    Technical,
}

/// Advanced skill extraction from interaction history
pub fn extract_skills_advanced(history: &[InteractionTurn]) -> Vec<ExtractedSkill> {
    let mut skills = Vec::new();
    let mut skill_scores = HashMap::new();

    // Analyze each turn for skill patterns
    for turn in history {
        let text = format!(
            "{} {}",
            turn.prompt,
            turn.answer.as_ref().unwrap_or(&String::new())
        );

        // Pattern-based skill extraction
        let turn_skills = analyze_turn_for_skills(&text, turn.turn_index);
        for skill in turn_skills {
            let entry = skill_scores
                .entry(skill.skill.clone())
                .or_insert((0.0, Vec::new()));
            entry.0 += skill.confidence;
            entry.1.extend(skill.evidence);
        }
    }

    // Convert to final skills with normalized confidence
    let max_score = skill_scores
        .values()
        .map(|(score, _)| *score)
        .max_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap_or(1.0);

    for (skill_name, (score, evidence)) in skill_scores {
        let category = infer_skill_category(&skill_name);
        skills.push(ExtractedSkill {
            skill: skill_name,
            confidence: score / max_score,
            evidence: evidence
                .into_iter()
                .collect::<HashSet<_>>()
                .into_iter()
                .collect(),
            category,
        });
    }

    // Sort by confidence
    skills.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
    skills
}

/// Analyze a single turn for skill patterns
fn analyze_turn_for_skills(text: &str, turn_index: usize) -> Vec<ExtractedSkill> {
    let mut skills = Vec::new();
    let text_lower = text.to_lowercase();

    // Domain-specific skills
    let domain_patterns = [
        (
            "investment-analysis",
            vec!["invest", "portfolio", "return", "risk", "diversif"],
            0.8,
        ),
        (
            "travel-planning",
            vec!["travel", "itinerary", "destination", "flight", "hotel"],
            0.8,
        ),
        (
            "research",
            vec!["research", "analyze", "study", "investigate", "explore"],
            0.7,
        ),
        (
            "project-management",
            vec!["project", "task", "deadline", "milestone", "deliverable"],
            0.8,
        ),
        (
            "financial-planning",
            vec!["budget", "finance", "saving", "expense", "income"],
            0.8,
        ),
        (
            "data-analysis",
            vec!["data", "analytics", "statistics", "trend", "pattern"],
            0.7,
        ),
    ];

    for (skill, keywords, base_confidence) in &domain_patterns {
        let matches = keywords.iter().filter(|&k| text_lower.contains(k)).count();
        if matches > 0 {
            let confidence = *base_confidence * (matches as f64 / keywords.len() as f64);
            skills.push(ExtractedSkill {
                skill: skill.to_string(),
                confidence,
                evidence: vec![format!("Turn {}: matched {} keywords", turn_index, matches)],
                category: SkillCategory::DomainSpecific,
            });
        }
    }

    // Orchestration skills
    if text_lower.contains("plan")
        && (text_lower.contains("step") || text_lower.contains("sequence"))
    {
        skills.push(ExtractedSkill {
            skill: "multi-step-orchestration".to_string(),
            confidence: 0.9,
            evidence: vec![format!("Turn {}: planning language detected", turn_index)],
            category: SkillCategory::Orchestration,
        });
    }

    // Analysis skills
    let analysis_indicators = ["analyze", "evaluate", "assess", "compare", "optimize"];
    let analysis_matches = analysis_indicators
        .iter()
        .filter(|&k| text_lower.contains(k))
        .count();
    if analysis_matches > 0 {
        skills.push(ExtractedSkill {
            skill: "analytical-reasoning".to_string(),
            confidence: 0.6 + (analysis_matches as f64 * 0.1),
            evidence: vec![format!(
                "Turn {}: {} analysis indicators",
                turn_index, analysis_matches
            )],
            category: SkillCategory::Analysis,
        });
    }

    // Communication skills
    if text_lower.contains("explain")
        || text_lower.contains("describe")
        || text_lower.contains("clarify")
    {
        skills.push(ExtractedSkill {
            skill: "explanatory-communication".to_string(),
            confidence: 0.7,
            evidence: vec![format!(
                "Turn {}: explanatory language detected",
                turn_index
            )],
            category: SkillCategory::Communication,
        });
    }

    skills
}

/// Infer skill category from skill name
fn infer_skill_category(skill_name: &str) -> SkillCategory {
    if skill_name.contains("analysis")
        || skill_name.contains("research")
        || skill_name.contains("evaluate")
    {
        SkillCategory::Analysis
    } else if skill_name.contains("orchestration")
        || skill_name.contains("planning")
        || skill_name.contains("management")
    {
        SkillCategory::Orchestration
    } else if skill_name.contains("communication") || skill_name.contains("explanatory") {
        SkillCategory::Communication
    } else if skill_name.contains("technical")
        || skill_name.contains("programming")
        || skill_name.contains("system")
    {
        SkillCategory::Technical
    } else {
        SkillCategory::DomainSpecific
    }
}

/// Extract constraints from interaction patterns
pub fn extract_constraints(history: &[InteractionTurn]) -> Vec<String> {
    let mut constraints = Vec::new();
    let mut seen = HashSet::new();

    for turn in history {
        let text = format!(
            "{} {}",
            turn.prompt,
            turn.answer.as_ref().unwrap_or(&String::new())
        )
        .to_lowercase();

        // Common constraint patterns
        let constraint_patterns = [
            (
                "time-sensitive",
                vec!["urgent", "deadline", "asap", "quickly", "fast"],
            ),
            (
                "high-precision",
                vec!["accurate", "precise", "exact", "detailed", "thorough"],
            ),
            (
                "cost-conscious",
                vec!["budget", "cheap", "affordable", "cost-effective"],
            ),
            (
                "risk-averse",
                vec!["safe", "conservative", "low-risk", "reliable"],
            ),
            ("eu-compliant", vec!["eu", "europe", "gdpr", "compliance"]),
            (
                "high-security",
                vec!["secure", "confidential", "private", "encrypted"],
            ),
        ];

        for (constraint, keywords) in &constraint_patterns {
            if keywords.iter().any(|k| text.contains(k)) && seen.insert(constraint.to_string()) {
                constraints.push(constraint.to_string());
            }
        }
    }

    constraints
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_extraction() {
        let history = vec![
            InteractionTurn {
                turn_index: 0,
                prompt: "What type of investment are you looking for?".to_string(),
                answer: Some("I want to invest in European stocks with low risk".to_string()),
            },
            InteractionTurn {
                turn_index: 1,
                prompt: "What's your risk tolerance?".to_string(),
                answer: Some("Low to medium, I prefer safe investments".to_string()),
            },
        ];

        let skills = extract_skills_advanced(&history);
        assert!(!skills.is_empty());
        assert!(skills
            .iter()
            .any(|s| s.skill.contains("investment") || s.skill.contains("risk")));
    }

    #[test]
    fn test_constraint_extraction() {
        let history = vec![InteractionTurn {
            turn_index: 0,
            prompt: "Any specific requirements?".to_string(),
            answer: Some("Must be EU compliant and cost-effective".to_string()),
        }];

        let constraints = extract_constraints(&history);
        assert!(constraints.contains(&"eu-compliant".to_string()));
        assert!(constraints.contains(&"cost-conscious".to_string()));
    }
}
