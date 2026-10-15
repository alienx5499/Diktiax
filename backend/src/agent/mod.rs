//! Autonomous investigation: a `Planner` decides graph tool calls, the `ClaimInvestigator`
//! executes them via MCP, scores detector evidence deterministically and dispatches actions.

mod claude_planner;
mod dossier;
mod evidence;
mod fallback_planner;
mod investigator;
mod openai_planner;
mod planner;
mod rule_planner;
mod verdict;

pub use claude_planner::{ClaudeConfig, ClaudePlanner};
pub use dossier::{AgentStep, InvestigationDossier};
pub use investigator::{ClaimInvestigator, PlannerConfig};
pub use openai_planner::{OpenAiConfig, OpenAiPlanner};
pub use planner::{PlannedCall, Planner, ToolOutcome, MAX_STEPS};
pub use rule_planner::RuleBasedPlanner;
