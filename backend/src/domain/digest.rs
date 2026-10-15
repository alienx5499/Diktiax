use super::entities::ProvenanceStep;
use sha2::{Digest, Sha256};

pub struct ProvenanceDigestSigner;

impl ProvenanceDigestSigner {
    /// Deterministically computes the SHA-256 digest of claim provenance steps.
    /// Time Complexity: O(M) where M is total canonical bytes of evidence.
    /// Space Complexity: O(1) auxiliary state in the SHA-256 register.
    pub fn compute_digest(claim_id: &str, steps: &[ProvenanceStep]) -> String {
        let mut hasher = Sha256::new();

        hasher.update(b"DIKTIAX_CANONICAL_PROVENANCE_V1:");
        hasher.update(claim_id.as_bytes());

        for step in steps {
            hasher.update(b"|STEP:");
            hasher.update(step.step_number.to_string().as_bytes());
            hasher.update(b":EXPR:");
            hasher.update(step.traversal_expression.as_bytes());
            hasher.update(b":RISK:");
            hasher.update(step.risk_delta.to_string().as_bytes());

            for node in &step.nodes_implicated {
                hasher.update(b":NODE:");
                hasher.update(node.as_bytes());
            }
        }

        let result = hasher.finalize();
        hex::encode(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_digest_deterministic_and_valid_hex() {
        let steps = vec![ProvenanceStep {
            step_number: 1,
            traversal_expression: "A -> B".into(),
            cypher_source: "MATCH (a)".into(),
            nodes_implicated: vec!["A".into(), "B".into()],
            finding: "Link discovered".into(),
            risk_delta: 25,
        }];

        let digest1 = ProvenanceDigestSigner::compute_digest("CLM-48291", &steps);
        let digest2 = ProvenanceDigestSigner::compute_digest("CLM-48291", &steps);

        assert_eq!(digest1, digest2);
        assert_eq!(digest1.len(), 64); // 256 bits = 64 hex characters
    }
}
