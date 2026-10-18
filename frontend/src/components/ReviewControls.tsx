'use client';

import React, { useState } from 'react';
import { InvestigationDossier, ReviewDecision, ReviewRecord, reviewClaim } from '@/lib/api';
import { ShieldAlert, Unlock } from 'lucide-react';

/** Investigator id from the ASSIGN_INVESTIGATOR action: "Assigned Omar Haddad (INV-202) - specialties: …". */
function assignedInvestigatorId(dossier: InvestigationDossier): string | null {
  const action = dossier.remediation.actions_log.find((a) => a.action_type === 'ASSIGN_INVESTIGATOR' && a.status === 'CONFIRMED');
  return action?.details.match(/^Assigned .* \(([^()]+)\) - specialties/)?.[1] ?? null;
}

const DECISION_TEXT: Record<ReviewDecision, string> = {
  confirm_fraud: 'confirmed fraud',
  release: 'released payout',
};

const BUTTON =
  'px-4 py-2 rounded-none text-xs font-black uppercase border-2 border-[#121212] shadow-[3px_3px_0px_0px_#121212] active:translate-x-[2px] active:translate-y-[2px] active:shadow-none transition-all flex items-center gap-2 cursor-pointer disabled:opacity-50';

const HELD_STATUSES = ['FLAGGED_SIU_FRAUD', 'PAYOUT_FROZEN', 'NEEDS_REVIEW_CONFLICTING_VERDICTS'];

/**
 * A frozen payout is a hold: the assigned investigator confirms the fraud or releases it.
 * Buttons appear only when the backend accepts reviews from this browser (`review_enabled`).
 */
export function ReviewControls({ dossier, enabled }: { dossier: InvestigationDossier; enabled: boolean }) {
  const [review, setReview] = useState<ReviewRecord | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const reviewer = assignedInvestigatorId(dossier);

  if (review) {
    return (
      <p data-testid="review-outcome" className="text-sm font-black border-2 border-[#121212] bg-[#F0F0F0] p-3">
        Reviewed by {review.reviewer_name}: {DECISION_TEXT[review.decision]}
      </p>
    );
  }
  if (!HELD_STATUSES.includes(dossier.status) || !reviewer) return null;
  if (!enabled) {
    return (
      <p data-testid="review-awaiting" className="text-sm font-black border-2 border-[#121212] bg-[#F0F0F0] p-3">
        Awaiting human review by {dossier.remediation.assigned_investigator}
      </p>
    );
  }

  const decide = async (decision: ReviewDecision) => {
    setPending(true);
    setError(null);
    try {
      setReview(await reviewClaim(dossier.claim_id, decision, reviewer));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setPending(false);
    }
  };

  return (
    <div data-testid="review-controls" className="space-y-2 pt-4 border-t-2 border-[#121212]">
      <span className="text-xs font-mono font-black uppercase tracking-wider block">
        Human review as {dossier.remediation.assigned_investigator}
      </span>
      <div className="flex flex-wrap gap-3">
        <button onClick={() => decide('confirm_fraud')} disabled={pending} className={`${BUTTON} bg-[#D02020] text-white`}>
          <ShieldAlert className="w-3.5 h-3.5" />
          <span>Confirm fraud</span>
        </button>
        <button onClick={() => decide('release')} disabled={pending} className={`${BUTTON} bg-white text-[#121212] hover:bg-[#F0C020]`}>
          <Unlock className="w-3.5 h-3.5" />
          <span>Release payout</span>
        </button>
      </div>
      {error && <p className="text-xs font-mono font-bold text-[#D02020]">{error}</p>}
    </div>
  );
}
