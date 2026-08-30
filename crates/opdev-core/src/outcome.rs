use serde::{Deserialize, Serialize};

/// Exhaustive result of evaluating one rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The rule applies and its required evidence is valid.
    Passed,
    /// Evidence demonstrates that the requirement is not met.
    Failed,
    /// The rule appears applicable but evidence or permission is missing.
    Unverified,
    /// The applicability condition is demonstrably false.
    NotApplicable,
    /// The verifier could not complete.
    Error,
    /// A brownfield gap is tracked but not yet satisfied.
    MigrationRequired,
}

impl Outcome {
    /// Returns whether this result satisfies a required rule during aggregation.
    #[must_use]
    pub const fn satisfies_required_rule(self) -> bool {
        matches!(self, Self::Passed | Self::NotApplicable)
    }
}

/// Aggregate decision for a set of required rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateVerdict {
    /// Every applicable required rule is satisfied.
    Passed,
    /// At least one required rule blocks the verdict.
    Blocked,
}

impl AggregateVerdict {
    /// Aggregates required rule outcomes without weakening unknown or error states.
    pub fn from_required(outcomes: impl IntoIterator<Item = Outcome>) -> Self {
        if outcomes.into_iter().all(Outcome::satisfies_required_rule) {
            Self::Passed
        } else {
            Self::Blocked
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_passed_and_not_applicable_satisfy_required_rules() {
        assert!(Outcome::Passed.satisfies_required_rule());
        assert!(Outcome::NotApplicable.satisfies_required_rule());
        assert!(!Outcome::Failed.satisfies_required_rule());
        assert!(!Outcome::Unverified.satisfies_required_rule());
        assert!(!Outcome::Error.satisfies_required_rule());
        assert!(!Outcome::MigrationRequired.satisfies_required_rule());
    }

    #[test]
    fn any_non_satisfying_outcome_blocks_aggregation() {
        let verdict = AggregateVerdict::from_required([
            Outcome::Passed,
            Outcome::NotApplicable,
            Outcome::Unverified,
        ]);
        assert_eq!(verdict, AggregateVerdict::Blocked);
    }
}
