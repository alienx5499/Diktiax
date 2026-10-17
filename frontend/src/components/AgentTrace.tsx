import React from 'react';
import { AgentStep, ProvenanceStep } from '@/lib/api';

/** Compact `key=value` rendering of tool arguments. */
const fmtArgs = (args: Record<string, unknown>) =>
  Object.entries(args ?? {})
    .map(([k, v]) => `${k}=${typeof v === 'string' ? v : JSON.stringify(v)}`)
    .join(', ');

interface AgentTraceProps {
  trace: AgentStep[];
  planner: string;
  /** certificate.topological_evidence; aligned by index with `trace` (as returned by the backend). */
  evidence?: ProvenanceStep[];
}

export function AgentTrace({ trace, planner, evidence = [] }: AgentTraceProps) {
  // Index alignment is by position in the original trace, so keep the index through the sort.
  const steps = trace.map((s, i) => ({ s, cypher: evidence[i]?.cypher_source ?? '' })).sort((a, b) => a.s.step - b.s.step);
  return (
    <div className="w-full bg-white border-2 lg:border-4 border-[#121212] shadow-[6px_6px_0px_0px_#121212] rounded-none p-6">
      <div className="flex items-center justify-between gap-3 pb-4 border-b-2 border-[#121212]">
        <h3 className="text-base font-black uppercase tracking-tight">Agent Reasoning Trace</h3>
        <span
          data-testid="planner-badge"
          className="text-xs font-mono font-black uppercase px-3 py-1 border-2 border-[#121212] bg-[#F0C020] shadow-[2px_2px_0px_0px_#121212]"
        >
          Planner: {planner}
        </span>
      </div>

      {steps.length === 0 ? (
        <p className="pt-4 text-xs font-mono font-bold uppercase text-[#121212]/70">No agent steps were recorded for this investigation.</p>
      ) : (
        <ol className="pt-4 space-y-3">
          {steps.map(({ s, cypher }) => (
            <li key={s.step} className="flex gap-3">
              <span className="w-8 h-8 shrink-0 flex items-center justify-center bg-[#1040C0] text-white border-2 border-[#121212] font-mono text-xs font-black">
                {s.step}
              </span>
              <div className="min-w-0 flex-1 p-3 bg-[#F0F0F0] border-2 border-[#121212] shadow-[2px_2px_0px_0px_#121212] space-y-1">
                <div className="flex items-center justify-between gap-2">
                  <span className="font-mono text-sm font-black uppercase break-all">{s.tool}</span>
                  <span className="font-mono text-xs font-black text-[#1040C0] shrink-0">{s.latency_ms.toFixed(2)} ms</span>
                </div>
                {fmtArgs(s.arguments) && (
                  <p className="font-mono text-xs font-bold text-[#1040C0] break-all" data-testid="trace-args">
                    {fmtArgs(s.arguments)}
                  </p>
                )}
                <p className="text-xs text-[#121212]/80">
                  <span className="font-black uppercase">Why: </span>
                  {s.rationale}
                </p>
                <p className="text-xs text-[#121212]">
                  <span className="font-black uppercase">Result: </span>
                  {s.result_summary}
                </p>
                {cypher && (
                  <details className="pt-1">
                    <summary className="cursor-pointer text-xs font-black uppercase focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#1040C0]">
                      Cypher
                    </summary>
                    <pre className="mt-1 p-2 bg-[#121212] text-[#F0F0F0] text-[11px] font-mono whitespace-pre-wrap break-all border-2 border-[#121212]">
                      {cypher}
                    </pre>
                  </details>
                )}
              </div>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}
