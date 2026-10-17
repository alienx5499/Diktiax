'use client';

import React from 'react';
import { InvestigationDossier } from '@/lib/api';
import { EvidenceCard } from '@/components/EvidenceCard';
import { ActionPanel } from '@/components/ActionPanel';

interface ForensicSectionsProps {
  dossier: InvestigationDossier;
  selectedClaimId: string;
  onReinvestigate: () => void;
  isInvestigating: boolean;
}

export function ForensicSections({ dossier, selectedClaimId, onReinvestigate, isInvestigating }: ForensicSectionsProps) {
  return (
    <div className="space-y-12">
      {/* STEP 1: FORENSIC EVIDENCE & PROVENANCE (FIRST — FULL WIDTH) */}
      <section className="space-y-5 pt-8 border-t-4 border-[#121212]">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 pb-2">
          <div className="flex items-center space-x-3.5">
            <span className="text-xs sm:text-sm font-black uppercase px-3.5 py-1.5 bg-[#1040C0] text-white border-2 lg:border-4 border-[#121212] shadow-[4px_4px_0px_0px_#121212] rounded-none">
              STEP 01
            </span>
            <div>
              <h2 className="text-xl sm:text-2xl lg:text-3xl font-black uppercase tracking-tight text-[#121212]">
                Cryptographic Evidence & Provenance Dossier
              </h2>
              <p className="text-xs font-bold uppercase tracking-wider text-[#121212]/70 font-mono">
                Digest of the claim and steps (re-hash a saved dossier to check it).
              </p>
            </div>
          </div>
          <span className="text-xs font-mono text-[#121212] font-black border-2 border-[#121212] shadow-[3px_3px_0px_0px_#121212] px-3.5 py-1.5 bg-[#F0C020] rounded-none self-start sm:self-auto">
            TARGET: <span>{selectedClaimId}</span>
          </span>
        </div>

        <EvidenceCard certificate={dossier.certificate} riskScore={dossier.risk_score} confidence={dossier.confidence} />
      </section>

      {/* STEP 2: AUTONOMOUS ENFORCEMENT & REMEDIATION (SECOND — FULL WIDTH) */}
      <section className="space-y-5 pt-8 border-t-4 border-[#121212]">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 pb-2">
          <div className="flex items-center space-x-3.5">
            <span className="text-xs sm:text-sm font-black uppercase px-3.5 py-1.5 bg-[#D02020] text-white border-2 lg:border-4 border-[#121212] shadow-[4px_4px_0px_0px_#121212] rounded-none">
              STEP 02
            </span>
            <div>
              <h2 className="text-xl sm:text-2xl lg:text-3xl font-black uppercase tracking-tight text-[#121212]">
                Policy Remediation & Automated Enforcement Center
              </h2>
              <p className="text-xs font-bold uppercase tracking-wider text-[#121212]/70 font-mono">
                Automated policy enforcement, escrow disbursement freeze, and investigator dispatch.
              </p>
            </div>
          </div>
          <span className="text-xs font-mono text-white font-black px-3.5 py-1.5 bg-[#D02020] border-2 border-[#121212] shadow-[3px_3px_0px_0px_#121212] rounded-none self-start sm:self-auto">
            {dossier.remediation.payout_frozen
              ? `DISBURSEMENT FROZEN ($${dossier.remediation.frozen_amount.toLocaleString()})`
              : 'DISBURSEMENT AUTHORIZED'}
          </span>
        </div>

        <ActionPanel
          remediation={dossier.remediation}
          riskScore={dossier.risk_score}
          claimId={dossier.claim_id}
          onReinvestigate={onReinvestigate}
          isInvestigating={isInvestigating}
        />
      </section>
    </div>
  );
}
