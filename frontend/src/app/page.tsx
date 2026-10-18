'use client';

import React, { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import {
  ClaimSummary,
  InvestigationDossier,
  InvestigationRecord,
  fetchClaims,
  fetchInvestigationHistory,
  investigateClaim,
  seedDatabase,
  checkBackendHealth,
  ApiError,
} from '@/lib/api';

import { Navbar } from '@/components/Navbar';
import { GraphCanvas } from '@/components/GraphCanvas';
import { ForensicSections } from '@/components/ForensicSections';
import { PatternFilter } from '@/components/PatternFilter';
import { AgentTrace } from '@/components/AgentTrace';
import { NewClaimForm } from '@/components/NewClaimForm';
import { ErrorPanel } from '@/components/ErrorPanel';
import { BootLoader } from '@/components/BootLoader';
import { Footer } from '@/components/Footer';
import { VerdictHeader } from '@/components/VerdictHeader';
import { MemoryPanel } from '@/components/MemoryPanel';

/** Demo scenario labels from the seed data; the ring is listed first, other claims keep API order. */
const RING_CLAIM = 'CLM-48291';
const CLAIM_LABELS: Record<string, string> = {
  [RING_CLAIM]: 'fraud ring',
  'CLM-20988': 'clean control',
  'CLM-10022': 'clean control',
  'CLM-20417': 'clean control',
};

const errMsg = (e: unknown) => (e instanceof Error ? e.message : String(e));

export default function Home() {
  const [claims, setClaims] = useState<ClaimSummary[]>([]);
  const [claimsError, setClaimsError] = useState<string | null>(null);
  const [selectedClaimId, setSelectedClaimId] = useState<string | null>(null);
  const [dossier, setDossier] = useState<InvestigationDossier | null>(null);
  const [investigateError, setInvestigateError] = useState<string | null>(null);
  const [isInvestigating, setIsInvestigating] = useState<boolean>(false);
  const [isReseeding, setIsReseeding] = useState<boolean>(false);
  const [engineOnline, setEngineOnline] = useState<boolean>(false);
  const [seedEnabled, setSeedEnabled] = useState<boolean>(false);
  const [reviewEnabled, setReviewEnabled] = useState<boolean>(false);
  const [activePattern, setActivePattern] = useState<string | null>(null);
  // True until the first claims request settles: covers a cold-starting backend.
  const [booting, setBooting] = useState<boolean>(true);
  /** null records = history endpoint unavailable (panel degrades quietly). */
  const [history, setHistory] = useState<{ claimId: string; records: InvestigationRecord[] | null } | null>(null);
  const [rateLimitMsg, setRateLimitMsg] = useState<string | null>(null);
  // Last successful result, restored when an investigation is rate limited.
  const lastGood = useRef<{ claimId: string; dossier: InvestigationDossier } | null>(null);

  const orderedClaims = useMemo(
    () => [...claims.filter((c) => c.claim_id === RING_CLAIM), ...claims.filter((c) => c.claim_id !== RING_CLAIM)],
    [claims],
  );

  const evidenceIds = useMemo(() => {
    const patterns = dossier?.patterns_matched ?? [];
    const picked = activePattern ? patterns.filter((p) => p.pattern_name === activePattern) : patterns;
    return new Set(picked.flatMap((p) => p.evidence_path ?? []));
  }, [dossier, activePattern]);

  const loadInvestigation = useCallback(async (claimId: string) => {
    setIsInvestigating(true);
    setSelectedClaimId(claimId);
    setInvestigateError(null);
    setRateLimitMsg(null);
    setDossier(null);
    setActivePattern(null);
    try {
      const result = await investigateClaim(claimId);
      lastGood.current = { claimId, dossier: result };
      setDossier(result);
      fetchInvestigationHistory(claimId).then(
        (records) => setHistory({ claimId, records }),
        () => setHistory({ claimId, records: null }),
      );
    } catch (err) {
      if (err instanceof ApiError && err.status === 429) {
        const wait = err.retryAfterSecs;
        setRateLimitMsg(`One investigation every 15 s per visitor. Try again${wait != null ? ` in ${wait} s` : ' shortly'}.`);
        setSelectedClaimId(lastGood.current?.claimId ?? null);
        setDossier(lastGood.current?.dossier ?? null);
      } else {
        setInvestigateError(errMsg(err));
      }
    } finally {
      setIsInvestigating(false);
    }
  }, []);

  const loadClaims = useCallback(async (): Promise<ClaimSummary[] | null> => {
    setClaimsError(null);
    try {
      const list = await fetchClaims();
      setClaims(list);
      setEngineOnline(true);
      return list;
    } catch (err) {
      setEngineOnline(false);
      setClaimsError(errMsg(err));
      return null; // surfaced via claimsError panel
    }
  }, []);

  useEffect(() => {
    checkBackendHealth().then((h) => {
      setSeedEnabled(h.seedEnabled);
      setReviewEnabled(h.reviewEnabled);
    });
    fetchClaims()
      .then(
        (list) => {
          setClaims(list);
          setEngineOnline(true);
        },
        (err) => {
          setEngineOnline(false);
          setClaimsError(errMsg(err));
        },
      )
      .finally(() => setBooting(false));
  }, []);

  const handleReseed = async () => {
    setIsReseeding(true);
    try {
      await seedDatabase();
      await loadClaims();
      if (selectedClaimId) await loadInvestigation(selectedClaimId);
    } catch (err) {
      setClaimsError(
        err instanceof ApiError && err.status === 403
          ? 'Reseed refused (HTTP 403): the backend only allows it with DIKTIAX_DEV=1 or an admin token.'
          : errMsg(err),
      );
    } finally {
      setIsReseeding(false);
    }
  };

  const handleCreated = async (created: ClaimSummary) => {
    await loadClaims();
    await loadInvestigation(created.claim_id);
  };

  return (
    <div className="min-h-screen w-full bg-[#F0F0F0] text-[#121212] font-sans flex flex-col antialiased">
      {booting && <BootLoader />}
      <Navbar engineOnline={engineOnline} onReseed={handleReseed} isReseeding={isReseeding} seedEnabled={seedEnabled} />

      <main className="flex-1 w-full max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8 space-y-10">
        {/* Bauhaus Constructivist Page Header */}
        <div className="space-y-4">
          <div className="flex flex-col lg:flex-row lg:items-end justify-between gap-6 pb-2">
            <div>
              <div className="flex items-center space-x-2 text-xs font-bold uppercase tracking-widest text-[#1040C0] mb-2">
                <span className="w-2.5 h-2.5 rounded-full bg-[#D02020] border border-[#121212]" />
                <span>Autonomous Insurance Fraud Detection</span>
              </div>
              <h1 className="text-4xl sm:text-5xl lg:text-7xl font-black uppercase tracking-tighter text-[#121212] leading-[0.9]">
                Forensic Graph Intelligence
              </h1>
              <p className="text-xs sm:text-sm font-medium uppercase tracking-wider text-[#121212]/80 mt-2.5">
                Deterministic topological reasoning & checkable evidence chain
              </p>
            </div>

            {/* Target Claim Selector Tabs */}
            <div className="space-y-1.5 self-start lg:self-auto">
              <span className="text-[11px] font-bold uppercase tracking-widest text-[#121212]/70 block">Select Forensic Target</span>
              <div className="flex items-center gap-2 overflow-x-auto pb-1">
                {claims.length === 0 && !claimsError && (
                  <span className="font-mono text-xs font-bold uppercase text-[#121212]/70">No claims yet. Submit one below.</span>
                )}
                {orderedClaims.map((claim) => {
                  const isSelected = claim.claim_id === selectedClaimId;
                  const label = CLAIM_LABELS[claim.claim_id];
                  return (
                    <button
                      key={claim.claim_id}
                      onClick={() => loadInvestigation(claim.claim_id)}
                      className={`px-4 py-2 font-mono text-xs font-black uppercase tracking-wider transition-all cursor-pointer rounded-none active:translate-x-[2px] active:translate-y-[2px] active:shadow-none ${
                        isSelected
                          ? 'bg-[#D02020] text-white border-2 lg:border-4 border-[#121212] shadow-[4px_4px_0px_0px_#121212]'
                          : 'bg-white text-[#121212] border-2 lg:border-4 border-[#121212] shadow-[3px_3px_0px_0px_#121212] hover:bg-[#F0C020]'
                      }`}
                    >
                      {claim.claim_id}
                      {label && (
                        <span className={`block text-[10px] tracking-widest ${isSelected ? 'text-white' : 'text-[#121212]/70'}`}>
                          {label}
                        </span>
                      )}
                    </button>
                  );
                })}
              </div>
            </div>
          </div>

          {/* Primary Bauhaus Decorative Color Block Strip */}
          <div className="flex h-3 w-full border-2 border-[#121212] shadow-[2px_2px_0px_0px_#121212]">
            <div className="w-1/3 bg-[#D02020]" />
            <div className="w-1/3 bg-[#1040C0]" />
            <div className="w-1/3 bg-[#F0C020]" />
          </div>
        </div>

        {claimsError && <ErrorPanel title="Backend unavailable: cannot load claims" message={claimsError} onRetry={() => loadClaims()} />}

        {investigateError && selectedClaimId && (
          <ErrorPanel
            title={`Investigation failed for ${selectedClaimId}`}
            message={investigateError}
            onRetry={() => loadInvestigation(selectedClaimId)}
          />
        )}

        {rateLimitMsg && (
          <div
            role="status"
            className="w-full bg-[#FFF9C4] border-2 lg:border-4 border-[#121212] shadow-[6px_6px_0px_0px_#121212] rounded-none p-4 font-mono text-xs font-black uppercase"
          >
            {rateLimitMsg}
          </div>
        )}

        {dossier && (
          <div className="grid grid-cols-1 lg:grid-cols-3 gap-6 items-start">
            <div className="lg:col-span-2">
              <VerdictHeader dossier={dossier} reviewEnabled={reviewEnabled} />
            </div>
            <MemoryPanel history={history?.claimId === dossier.claim_id ? history.records : null} memory={dossier.memory} />
          </div>
        )}

        {/* Subgraph Canvas Section */}
        {selectedClaimId && !investigateError && (
          <section className="w-full space-y-3">
            {dossier && <PatternFilter patterns={dossier.patterns_matched ?? []} active={activePattern} onChange={setActivePattern} />}
            {dossier ? (
              <GraphCanvas
                subgraph={dossier.subgraph}
                selectedClaimId={selectedClaimId}
                isInvestigating={isInvestigating}
                evidenceIds={evidenceIds}
              />
            ) : (
              <div
                role="status"
                className="h-[540px] w-full bg-white border-2 lg:border-4 border-[#121212] shadow-[6px_6px_0px_0px_#121212] rounded-none flex items-center justify-center font-mono font-bold uppercase text-xs text-[#121212]"
              >
                Agent investigating graph...
              </div>
            )}
          </section>
        )}

        {dossier && (
          <AgentTrace trace={dossier.agent_trace} planner={dossier.planner} evidence={dossier.certificate.topological_evidence} />
        )}

        {/* Step 01 Evidence & Step 02 Enforcement */}
        {dossier && selectedClaimId && (
          <ForensicSections
            dossier={dossier}
            selectedClaimId={selectedClaimId}
            onReinvestigate={() => loadInvestigation(selectedClaimId)}
            isInvestigating={isInvestigating}
          />
        )}

        <NewClaimForm onCreated={handleCreated} />
      </main>

      <Footer />
    </div>
  );
}
