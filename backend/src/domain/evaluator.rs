use super::entities::FraudPatternMatch;

pub struct FraudRiskAssessment {
    pub risk_score: i32,
    pub risk_tier: String,
    pub confidence: f64,
}

pub struct FraudRiskEvaluator;

impl FraudRiskEvaluator {
    /// Pure functional calculation: aggregates risk points, caps at 100, and classifies tier.
    pub fn evaluate(patterns: &[FraudPatternMatch]) -> FraudRiskAssessment {
        let raw_points: i32 = patterns.iter().map(|p| p.risk_points).sum();
        let risk_score = raw_points.clamp(0, 100);

        let (risk_tier, confidence) = if risk_score >= 80 {
            ("CRITICAL_FRAUD".to_string(), 0.98)
        } else if risk_score >= 50 {
            ("HIGH_SUSPICION".to_string(), 0.90)
        } else if risk_score >= 25 {
            ("MEDIUM_RISK".to_string(), 0.85)
        } else {
            ("LOW_RISK".to_string(), 0.99)
        };

        FraudRiskAssessment {
            risk_score,
            risk_tier,
            confidence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_patterns_evaluates_to_low_risk() {
        let assessment = FraudRiskEvaluator::evaluate(&[]);
        assert_eq!(assessment.risk_score, 0);
        assert_eq!(assessment.risk_tier, "LOW_RISK");
    }

    #[test]
    fn test_high_points_caps_at_100_critical() {
        let patterns = vec![
            FraudPatternMatch {
                pattern_name: "Test Ring".into(),
                severity: "CRITICAL".into(),
                risk_points: 60,
                description: "test".into(),
                cypher_query: "test".into(),
                evidence_path: vec![],
            },
            FraudPatternMatch {
                pattern_name: "Test Centrality".into(),
                severity: "HIGH".into(),
                risk_points: 50,
                description: "test".into(),
                cypher_query: "test".into(),
                evidence_path: vec![],
            },
        ];
        let assessment = FraudRiskEvaluator::evaluate(&patterns);
        assert_eq!(assessment.risk_score, 100);
        assert_eq!(assessment.risk_tier, "CRITICAL_FRAUD");
    }
}
