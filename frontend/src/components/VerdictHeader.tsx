import React from 'react';
import { InvestigationDossier } from '@/lib/api';
import { Lock, Unlock, UserCheck } from 'lucide-react';

/** Same thresholds EvidenceCard uses for its decision badge. */
export const riskColour = (score: number) =>
  score >= 70 ? 'bg-[#D02020] text-white' : score >= 40 ? 'bg-[#F0C020] text-[#121212]' : 'bg-[#1040C0] text-white';

export function VerdictHeader({ dossier }: { dossier: InvestigationDossier }) {
  const { remediation: r } = dossier;
  return (
    <div
      data-testid="verdict-header"
      className="w-full bg-white border-2 lg:border-4 border-[#121212] shadow-[6px_6px_0px_0px_#121212] lg:shadow-[8px_8px_0px_0px_#121212] rounded-none p-6 space-y-5"
    >
      <div className="flex flex-wrap items-center justify-between gap-3 pb-4 border-b-2 border-[#121212]">
        <div>
          <span className="text-xs font-mono font-black uppercase tracking-widest text-[#121212]/70 block">Verdict</span>
          <h2 className="text-2xl sm:text-3xl font-black uppercase tracking-tight">{dossier.claim_id}</h2>
        </div>
        <span
          className={`text-sm font-mono font-black uppercase px-4 py-1.5 border-2 lg:border-4 border-[#121212] shadow-[4px_4px_0px_0px_#121212] ${riskColour(dossier.risk_score)}`}
        >
          {dossier.risk_tier.replace(/_/g, ' ')}
        </span>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
        <div className="border-2 border-[#121212] shadow-[3px_3px_0px_0px_#121212] p-4 space-y-1">
          <span className="text-xs font-mono font-black uppercase tracking-wider block">Risk score</span>
          <p className="font-black tabular">
            <span className="text-5xl">{dossier.risk_score}</span>
            <span className="text-lg text-[#121212]/70"> / 100</span>
          </p>
        </div>

        <div className="border-2 border-[#121212] shadow-[3px_3px_0px_0px_#121212] p-4 space-y-1">
          <span className="text-xs font-mono font-black uppercase tracking-wider flex items-center gap-1.5">
            {r.payout_frozen ? <Lock className="w-3.5 h-3.5" /> : <Unlock className="w-3.5 h-3.5" />}
            Payout
          </span>
          <p className={`text-xl font-black uppercase ${r.payout_frozen ? 'text-[#D02020]' : 'text-[#1040C0]'}`}>
            {r.payout_frozen ? 'Frozen' : 'Not frozen'}
          </p>
          <p className="text-xs font-mono font-bold tabular">
            ${(r.payout_frozen ? r.frozen_amount : dossier.claim_amount).toLocaleString()}
          </p>
        </div>

        <div className="border-2 border-[#121212] shadow-[3px_3px_0px_0px_#121212] p-4 space-y-1">
          <span className="text-xs font-mono font-black uppercase tracking-wider flex items-center gap-1.5">
            <UserCheck className="w-3.5 h-3.5" />
            Investigator
          </span>
          <p className="text-xl font-black uppercase break-words">{r.assigned_investigator || 'None assigned'}</p>
          {r.investigator_role && <p className="text-xs font-mono font-bold uppercase">{r.investigator_role}</p>}
        </div>
      </div>
    </div>
  );
}
