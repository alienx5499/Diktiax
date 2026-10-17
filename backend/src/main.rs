use claimgraph_backend::actions::{ActionDispatcher, Webhook};
use claimgraph_backend::agent::{ClaimInvestigator, ClaudeConfig, OpenAiConfig, PlannerConfig};
use claimgraph_backend::api::{create_router, AppState, Cooldown, RING_COMPONENTS_COOLDOWN_SECS};
use claimgraph_backend::config::AppConfig;
use claimgraph_backend::db::FalkorGraphRepository;
use claimgraph_backend::mcp;
use std::net::SocketAddr;
use tracing::info;

const USAGE: &str = "usage: claimgraph-backend [--seed | --investigate <CLAIM_ID> | --mcp-server]";

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // stderr only: stdout carries the MCP protocol and --investigate JSON.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "claimgraph_backend=info,tower_http=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let config = AppConfig::from_env();
    let repo = FalkorGraphRepository::connect(&config.falkordb_url, &config.graph_name).await?;
    // Precedence: ANTHROPIC_API_KEY -> Claude, else GROQ_API_KEY -> OpenAI-compatible, else rules.
    let planner = match (ClaudeConfig::from_env()?, OpenAiConfig::from_env()?) {
        (Some(claude), _) => PlannerConfig::Claude(claude),
        (None, Some(openai)) => PlannerConfig::OpenAi(openai),
        (None, None) => PlannerConfig::RuleBased,
    };
    let dispatcher = ActionDispatcher::new(repo.clone(), Webhook::from_env()?);
    let investigator = ClaimInvestigator::new(repo.clone(), dispatcher, planner);
    info!("Planner: {}", investigator.planner_label());

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--seed") => {
            let report = repo.seed(true).await?;
            info!("Seeded: {report:?}");
        }
        Some("--investigate") => {
            let target = args.get(1).ok_or(USAGE)?;
            let dossier = investigator.investigate(target).await?;
            println!("{}", serde_json::to_string_pretty(&dossier)?);
        }
        Some("--mcp-server") => {
            repo.seed(false).await?;
            mcp::serve_stdio(repo, investigator).await?;
        }
        Some(other) => return Err(format!("unknown argument '{other}'. {USAGE}").into()),
        None => {
            let report = repo.seed(false).await?;
            info!("Graph ready: {report:?}");
            let addr: SocketAddr =
                format!("{}:{}", config.server_host, config.server_port).parse()?;
            let listener = tokio::net::TcpListener::bind(&addr).await?;
            info!("Listening on http://{addr}");
            let state = AppState {
                repo,
                investigator,
                investigate_cooldown: Cooldown::from_env(),
                ring_cooldown: Cooldown::new(RING_COMPONENTS_COOLDOWN_SECS),
            };
            let app = create_router(state).into_make_service_with_connect_info::<SocketAddr>();
            axum::serve(listener, app).await?;
        }
    }
    Ok(())
}
