'use client';

import React, { useState } from 'react';
import { TriageRemediationResult } from '@/lib/api';
import confetti from 'canvas-confetti';
import { Lock, UserCheck, Send, CheckCircle, RefreshCw, ArrowDownToLine, ArrowUpRight, Building } from 'lucide-react';

interface ActionPanelProps {
  remediation: TriageRemediationResult;
  riskScore: number;
  claimId: string;
  onReinvestigate: () => void;
  isInvestigating: boolean;
}

export function ActionPanel({ remediation, riskScore, claimId, onReinvestigate, isInvestigating }: ActionPanelProps) {
  const [overrideState, setOverrideState] = useState<'default' | 'overridden'>('default');
  const [showPayload, setShowPayload] = useState(false);
  const isFrozen = remediation.payout_frozen && overrideState !== 'overridden';

  const handleExportDossier = () => {
    confetti({
      particleCount: 60,
      spread: 70,
      origin: { y: 0.7 },
      colors: ['#D02020', '#1040C0', '#F0C020', '#121212'],
    });

    const blob = new Blob([JSON.stringify(remediation, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `diktiax-dossier-${claimId}.json`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="w-full bg-white border-2 lg:border-4 border-[#121212] shadow-[6px_6px_0px_0px_#121212] lg:shadow-[8px_8px_0px_0px_#121212] rounded-none p-6 sm:p-7 relative transition-transform duration-200 hover:-translate-y-1">
      {/* Top-Right Bauhaus Geometric Accent */}
      <div className="absolute top-4 right-4 flex items-center space-x-1" aria-hidden="true">
        <div className="w-3.5 h-3.5 rounded-none bg-[#1040C0] border border-[#121212]" />
        <div className="w-3.5 h-3.5 rounded-full bg-[#D02020] border border-[#121212]" />
        <div className="w-3.5 h-3.5 bg-[#F0C020] border border-[#121212]" style={{ clipPath: 'polygon(50% 0%, 0% 100%, 100% 100%)' }} />
      </div>

      <div className="space-y-6">
        {/* Card Header */}
        <div className="flex flex-col sm:flex-row sm:items-center justify-between pb-4 border-b-2 border-[#121212] gap-3">
          <div className="flex items-center space-x-3">
            <div className="w-10 h-10 rounded-none bg-[#D02020] text-white border-2 border-[#121212] shadow-[2px_2px_0px_0px_#121212] flex items-center justify-center">
              <Lock className="w-5 h-5 text-white" />
            </div>
            <div>
              <h3 className="text-base font-black uppercase tracking-tight text-[#121212]">Autonomous Enforcement Center</h3>
              <p className="text-xs font-bold uppercase tracking-wider text-[#121212]/70 mt-0.5">
                Real-time policy remediation & disbursement lockdown
              </p>
            </div>
          </div>

          <button
            onClick={onReinvestigate}
            disabled={isInvestigating}
            className="px-4 py-2 rounded-none text-xs font-black uppercase text-[#121212] bg-white hover:bg-[#F0C020] border-2 border-[#121212] shadow-[3px_3px_0px_0px_#121212] active:translate-x-[2px] active:translate-y-[2px] active:shadow-none transition-all flex items-center gap-2 cursor-pointer disabled:opacity-50 self-start sm:self-auto"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${isInvestigating ? 'animate-spin' : ''}`} />
            <span>Re-Verify Claim</span>
          </button>
        </div>

        {/* Action Status Cards: Bauhaus Primary Color-Blocked 3-Column Grid */}
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          {/* Action 1: ACH Escrow Lock (Red Block) */}
          <div className="p-4 rounded-none bg-[#FFF5F5] border-2 border-[#121212] shadow-[4px_4px_0px_0px_#121212] flex flex-col justify-between gap-3">
            <div className="flex items-start space-x-3">
              <div className="w-9 h-9 rounded-none bg-[#D02020] text-white border-2 border-[#121212] flex items-center justify-center shrink-0">
                <Building className="w-4 h-4" />
              </div>
              <div className="space-y-1">
                <span className="text-sm font-black uppercase text-[#121212] block">ACH Settlement Escrow Lock</span>
                <span className="text-xs text-[#121212]/80 leading-normal block">
                  {isFrozen
                    ? `Disbursement intercepted: $${remediation.frozen_amount.toLocaleString()} in forensic escrow.`
                    : 'Straight-through processing authorized.'}
                </span>
              </div>
            </div>
            <span className="text-xs font-mono font-black uppercase px-2.5 py-1 rounded-none border-2 border-[#121212] self-start bg-[#D02020] text-white">
              {isFrozen ? 'FROZEN IN ESCROW' : 'AUTHORIZED'}
            </span>
          </div>

          {/* Action 2: SIU Detective Assignment (Blue Block) */}
          <div className="p-4 rounded-none bg-[#F0F5FF] border-2 border-[#121212] shadow-[4px_4px_0px_0px_#121212] flex flex-col justify-between gap-3">
            <div className="flex items-start space-x-3">
              <div className="w-9 h-9 rounded-none bg-[#1040C0] text-white border-2 border-[#121212] flex items-center justify-center shrink-0">
                <UserCheck className="w-4 h-4" />
              </div>
              <div className="space-y-1">
                <span className="text-sm font-black uppercase text-[#121212] block">Special Investigations Unit (SIU)</span>
                <div className="text-xs text-[#121212] font-black uppercase">{remediation.assigned_investigator}</div>
                <div className="text-xs text-[#121212]/70 font-mono">{remediation.investigator_role}</div>
              </div>
            </div>
            <span className="text-xs font-mono font-black uppercase px-2.5 py-1 rounded-none bg-[#1040C0] text-white border-2 border-[#121212] self-start">
              AUTO-ASSIGNED
            </span>
          </div>

          {/* Action 3: Regulatory Fraud Webhook (Yellow Block) */}
          <div className="p-4 rounded-none bg-[#FFFDF0] border-2 border-[#121212] shadow-[4px_4px_0px_0px_#121212] flex flex-col justify-between gap-3">
            <div className="flex items-start space-x-3">
              <div className="w-9 h-9 rounded-none bg-[#F0C020] text-[#121212] border-2 border-[#121212] flex items-center justify-center shrink-0">
                <Send className="w-4 h-4" />
              </div>
              <div className="min-w-0 space-y-1">
                <span className="text-sm font-black uppercase text-[#121212] block">Alert Broadcast Webhook</span>
                <span className="text-xs text-[#121212]/80 font-mono truncate block">{remediation.webhook_target}</span>
              </div>
            </div>
            <div className="flex items-center justify-between">
              <button
                onClick={() => setShowPayload(!showPayload)}
                className="text-xs text-[#121212] hover:text-[#D02020] font-mono font-black uppercase underline cursor-pointer flex items-center gap-1"
              >
                <span>{showPayload ? 'Collapse' : 'Inspect JSON'}</span>
                <ArrowUpRight className="w-3.5 h-3.5" />
              </button>
              <span className="text-xs font-mono font-black uppercase px-2 py-0.5 rounded-none bg-[#121212] text-white border-2 border-[#121212]">
                {remediation.webhook_dispatched ? 'DISPATCHED' : 'NOT DISPATCHED'}
              </span>
            </div>
          </div>
        </div>

        {/* Formatted JSON Payload Preview */}
        {showPayload && (
          <pre className="p-4 rounded-none bg-[#121212] border-2 border-[#121212] text-xs font-mono text-white overflow-x-auto leading-relaxed">
            {JSON.stringify(
              {
                claim_id: claimId,
                risk_score: riskScore,
                payout_frozen: isFrozen,
                assigned_officer: remediation.assigned_investigator,
                webhook_target: remediation.webhook_target,
                webhook_dispatched: remediation.webhook_dispatched,
              },
              null,
              2,
            )}
          </pre>
        )}

        {/* Cryptographic Execution Audit Log */}
        <div className="space-y-2.5">
          <span className="text-xs font-mono uppercase tracking-wider text-[#121212] font-black block">
            Cryptographic Execution Audit Log ({remediation.actions_log.length} Events)
          </span>
          <div className="space-y-2 font-mono text-xs">
            {remediation.actions_log.map((act) => (
              <div
                key={act.id}
                className="p-3 rounded-none bg-[#F0F0F0] border-2 border-[#121212] shadow-[2px_2px_0px_0px_#121212] flex items-start justify-between gap-3 text-[#121212]"
              >
                <div className="flex items-start space-x-2.5">
                  <CheckCircle className="w-4 h-4 text-[#1040C0] shrink-0 mt-0.5" />
                  <div className="space-y-0.5">
                    <span className="text-[#121212] font-black uppercase">{act.action_type}: </span>
                    <span className="text-[#121212]/80 font-medium">{act.details}</span>
                  </div>
                </div>
                <span className="text-xs text-[#1040C0] font-black uppercase shrink-0">{act.status}</span>
              </div>
            ))}
          </div>
        </div>
      </div>

      {/* Footer Controls */}
      <div className="pt-5 border-t-2 border-[#121212] flex items-center justify-between mt-6">
        <button
          onClick={() => setOverrideState(overrideState === 'overridden' ? 'default' : 'overridden')}
          className="text-xs font-mono font-black uppercase text-[#121212] hover:text-[#D02020] cursor-pointer underline transition-colors"
        >
          {overrideState === 'overridden' ? 'Restore Autonomous Agent Lock' : 'Manual Supervisor Override'}
        </button>
        <button
          onClick={handleExportDossier}
          className="h-11 px-6 rounded-none text-xs font-black uppercase tracking-wider text-white bg-[#D02020] border-2 lg:border-4 border-[#121212] shadow-[4px_4px_0px_0px_#121212] hover:bg-[#D02020]/90 active:translate-x-[2px] active:translate-y-[2px] active:shadow-none transition-all flex items-center space-x-2 cursor-pointer"
        >
          <ArrowDownToLine className="w-4 h-4" />
          <span>Export Forensic Dossier</span>
        </button>
      </div>
    </div>
  );
}
