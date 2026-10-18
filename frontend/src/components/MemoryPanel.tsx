import React from 'react';
import { InvestigationMemory, InvestigationRecord } from '@/lib/api';
import { History } from 'lucide-react';
import { riskColour } from '@/components/VerdictHeader';

const shortCert = (id: string) => (id.length > 14 ? `${id.slice(0, 13)}…` : id);

const rtf = new Intl.RelativeTimeFormat('en', { numeric: 'auto' });
function relativeTime(iso: string): string {
  const secs = (new Date(iso).getTime() - Date.now()) / 1000;
  if (!Number.isFinite(secs)) return iso;
  const units: [Intl.RelativeTimeFormatUnit, number][] = [
    ['day', 86400],
    ['hour', 3600],
    ['minute', 60],
  ];
  for (const [unit, size] of units) if (Math.abs(secs) >= size) return rtf.format(Math.round(secs / size), unit);
  return rtf.format(Math.round(secs), 'second');
}

interface MemoryPanelProps {
  /** Newest first; null when the history endpoint was unavailable. */
  history: InvestigationRecord[] | null;
  memory?: InvestigationMemory;
}

export function MemoryPanel({ history, memory }: MemoryPanelProps) {
  if (!history && !memory) return null;
  // The run just finished is already in the history, so one entry and no memory means a first run.
  const first = !memory && (history?.length ?? 0) <= 1;
  const skipped = memory?.skipped_corroboration.length ?? 0;

  return (
    <div
      data-testid="memory-panel"
      className="w-full bg-white border-2 lg:border-4 border-[#121212] shadow-[6px_6px_0px_0px_#121212] rounded-none p-6 space-y-4"
    >
      <div className="flex items-center gap-2 pb-4 border-b-2 border-[#121212]">
        <History className="w-4 h-4" />
        <h3 className="text-base font-black uppercase tracking-tight">Investigation Memory</h3>
      </div>

      {memory && (
        <div className="space-y-1.5 text-sm font-medium bg-[#FFF9C4] p-3 border-2 border-[#121212] border-l-4 border-l-[#1040C0]">
          <p className="font-black">
            Memory: {memory.agrees_with_latest ? 'agrees with' : 'differs from'} the earlier verdict{' '}
            <span className="font-mono">{shortCert(memory.latest_prior.certificate_id)}</span>
          </p>
          {skipped > 0 && (
            <p className="text-xs font-mono font-bold">
              Skipped {skipped} corroboration check{skipped === 1 ? '' : 's'} already covered
            </p>
          )}
        </div>
      )}

      {first && <p className="text-xs font-mono font-black uppercase">First investigation of this claim</p>}

      {history && history.length > 0 && (
        <ol className="space-y-2 list-none p-0 m-0">
          {history.map((rec) => (
            <li
              key={rec.certificate_id}
              className="flex flex-wrap items-center justify-between gap-2 border-2 border-[#121212] shadow-[2px_2px_0px_0px_#121212] p-2.5"
            >
              <span className="font-mono text-xs font-black" title={rec.certificate_id}>
                {shortCert(rec.certificate_id)}
              </span>
              <span className="flex items-center gap-2">
                <span
                  className={`text-[11px] font-mono font-black uppercase px-2 py-0.5 border-2 border-[#121212] ${riskColour(rec.risk_score)}`}
                >
                  {rec.decision.replace(/_/g, ' ')} · {rec.risk_score}
                </span>
                <time dateTime={rec.recorded_at} className="text-[11px] font-mono font-bold text-[#121212]/70">
                  {relativeTime(rec.recorded_at)}
                </time>
              </span>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}
