export interface ClaimSummary {
  claim_id: string;
  claimant_name: string;
  amount: number;
  status: string;
  filed_at: string;
}

export interface NewClaim {
  claim_id?: string;
  claimant_name: string;
  phone: string;
  address: string;
  vehicle_vin: string;
  location_id: string;
  repair_shop_id: string;
  clinic_id?: string;
  adjuster_id?: string;
  amount: number;
  police_report: boolean;
}

export interface GraphNode {
  id: string;
  label: string;
  title: string;
  group: string;
  properties: Record<string, unknown>;
  x?: number;
  y?: number;
  vx?: number;
  vy?: number;
  isImplicated?: boolean;
}

export interface GraphEdge {
  id: string;
  from: string;
  to: string;
  label: string;
  properties: Record<string, unknown>;
  isSuspicious?: boolean;
}

export interface SubgraphPayload {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export interface ProvenanceStep {
  step_number: number;
  traversal_expression: string;
  cypher_source: string;
  nodes_implicated: string[];
  finding: string;
  risk_delta: number;
}

export interface ProvenanceCertificate {
  certificate_id: string;
  claim_id: string;
  generated_at: string;
  overall_decision: string;
  confidence_score: number;
  final_risk_score: number;
  execution_latency_ms: number;
  topological_evidence: ProvenanceStep[];
  forensic_conclusion: string;
  cryptographic_digest: string;
}

export interface ActionRecord {
  id: string;
  claim_id: string;
  action_type: string;
  details: string;
  executed_at: string;
  status: string;
  operator: string;
}

export interface TriageRemediationResult {
  claim_id: string;
  payout_frozen: boolean;
  frozen_amount: number;
  assigned_investigator: string;
  investigator_role: string;
  webhook_dispatched: boolean;
  webhook_target: string;
  actions_log: ActionRecord[];
}

export interface FraudPatternMatch {
  pattern_name: string;
  severity: string;
  risk_points: number;
  description: string;
  cypher_query: string;
  evidence_path: string[];
}

export interface AgentStep {
  step: number;
  tool: string;
  arguments: Record<string, unknown>;
  rationale: string;
  result_summary: string;
  latency_ms: number;
}

export interface InvestigationDossier {
  claim_id: string;
  claim_amount: number;
  status: string;
  risk_score: number;
  risk_tier: string;
  confidence: number;
  summary_rationale: string;
  patterns_matched: FraudPatternMatch[];
  certificate: ProvenanceCertificate;
  remediation: TriageRemediationResult;
  subgraph: SubgraphPayload;
  agent_trace: AgentStep[];
  /** "claude:<model>" | "rule-based" */
  planner: string;
  /** Omitted when the claim had no earlier investigation. */
  memory?: InvestigationMemory;
}

/** One entry of GET /api/claims/{id}/investigations (SUPERSEDES-chain order, newest first). */
export interface InvestigationRecord {
  certificate_id: string;
  decision: string;
  risk_score: number;
  recorded_at: string;
  /** History only: certificate id of the verdict this one superseded, null at the root. */
  supersedes?: string | null;
  /** History only: linked claim ids this verdict relied on. */
  relied_on?: string[];
  /** History only: 0 = newest. */
  position?: number;
}

export interface InvestigationMemory {
  prior_investigations: number;
  latest_prior: InvestigationRecord;
  agrees_with_latest: boolean;
  skipped_corroboration: string[];
  tool_calls_saved: number;
  /** Present (true) only when this run's tier differs from the latest verdict. */
  escalated?: boolean;
  escalation_reason?: string;
}

export type ReviewDecision = 'confirm_fraud' | 'release';

/** Response of POST /api/claims/{id}/review. */
export interface ReviewRecord {
  review_id: string;
  claim_id: string;
  decision: ReviewDecision;
  reviewer_id: string;
  reviewer_name: string;
  note?: string;
  reviewed_at: string;
  investigation_id: string;
  claim_status: string;
  payout_frozen: boolean;
}

export const API_BASE = process.env.NEXT_PUBLIC_API_BASE || 'http://127.0.0.1:3001';

/** Thrown for network failures (status 0) and non-2xx responses. Never masked by fake data. */
export class ApiError extends Error {
  constructor(
    message: string,
    public readonly status: number,
    public readonly offline: boolean,
    /** Seconds to wait, set on HTTP 429. */
    public readonly retryAfterSecs?: number,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

// The backend can take up to a minute to wake from idle; wait that long before giving up.
const DEFAULT_TIMEOUT_MS = 70000;

async function request<T>(path: string, init: RequestInit = {}, timeoutMs = DEFAULT_TIMEOUT_MS): Promise<T> {
  let res: Response;
  try {
    res = await fetch(`${API_BASE}${path}`, {
      ...init,
      cache: 'no-store',
      signal: AbortSignal.timeout(timeoutMs),
    });
  } catch (err) {
    const reason = err instanceof Error ? err.message : String(err);
    throw new ApiError(`Backend unreachable at ${API_BASE} (${reason})`, 0, true);
  }
  if (!res.ok) {
    const body = await res.text().catch(() => '');
    throw new ApiError(
      `Backend returned HTTP ${res.status}${body ? `: ${body.slice(0, 200)}` : ''}`,
      res.status,
      false,
      res.status === 429 ? retryAfterSecs(res, body) : undefined,
    );
  }
  return (await res.json()) as T;
}

/**
 * Retry-After is not a CORS-safelisted header, so cross-origin it reads as null unless the
 * backend exposes it; fall back to the "Try again in N s" text in the body.
 */
function retryAfterSecs(res: Response, body: string): number | undefined {
  const raw = res.headers.get('Retry-After') ?? body.match(/try again in (\d+)\s*s/i)?.[1];
  const n = Number(raw);
  return raw != null && Number.isFinite(n) ? Math.max(0, Math.ceil(n)) : undefined;
}

const JSON_HEADERS = { 'Content-Type': 'application/json' };

export interface BackendHealth {
  online: boolean;
  message: string;
  latencyMs: number;
  /** True only when POST /api/seed is allowed without an admin token (DIKTIAX_DEV=1). */
  seedEnabled: boolean;
  /** True only when POST /api/claims/{id}/review is allowed without an admin token. */
  reviewEnabled: boolean;
}

export async function checkBackendHealth(): Promise<BackendHealth> {
  const start = performance.now();
  try {
    const h = await request<{ seed_enabled?: boolean; review_enabled?: boolean }>('/api/health', {}, 1500);
    const latencyMs = Math.round(performance.now() - start);
    return {
      online: true,
      message: `Backend live (${latencyMs}ms)`,
      latencyMs,
      seedEnabled: h.seed_enabled === true,
      reviewEnabled: h.review_enabled === true,
    };
  } catch (err) {
    const message = err instanceof Error ? err.message : 'Backend unavailable';
    return { online: false, message, latencyMs: Math.round(performance.now() - start), seedEnabled: false, reviewEnabled: false };
  }
}

export const fetchClaims = () => request<ClaimSummary[]>('/api/claims');

export const createClaim = (claim: NewClaim) =>
  request<ClaimSummary>('/api/claims', { method: 'POST', headers: JSON_HEADERS, body: JSON.stringify(claim) });

export const investigateClaim = (claimId: string) =>
  request<InvestigationDossier>(`/api/investigate/${encodeURIComponent(claimId)}`, { method: 'POST', headers: JSON_HEADERS }, 150000);

export const fetchInvestigationHistory = (claimId: string) =>
  request<InvestigationRecord[]>(`/api/claims/${encodeURIComponent(claimId)}/investigations`, {}, 15000);

export const reviewClaim = (claimId: string, decision: ReviewDecision, reviewer: string) =>
  request<ReviewRecord>(
    `/api/claims/${encodeURIComponent(claimId)}/review`,
    { method: 'POST', headers: JSON_HEADERS, body: JSON.stringify({ decision, reviewer }) },
    15000,
  );

export const seedDatabase = () => request<{ status: string; message: string }>('/api/seed', { method: 'POST' });
