'use client';

import React, { useState } from 'react';
import { ProvenanceCertificate } from '@/lib/api';
import { Copy, Check, ChevronDown, ChevronUp, Terminal, Clock, Fingerprint } from 'lucide-react';

interface EvidenceCardProps {
  certificate: ProvenanceCertificate;
  riskScore: number;
  confidence: number;
}

export function EvidenceCard({
  certificate,
  confidence,
}: EvidenceCardProps) {
  const [copied, setCopied] = useState(false);
  const [expandedStep, setExpandedStep] = useState<number | null>(1);

  const handleCopyDigest = () => {
    navigator.clipboard.writeText(certificate.cryptographic_digest);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const isHighRisk = certificate.final_risk_score >= 70;

  return (
    <div className="w-full bg-white border-2 lg:border-4 border-[#121212] shadow-[6px_6px_0px_0px_#121212] lg:shadow-[8px_8px_0px_0px_#121212] rounded-none p-6 sm:p-7 relative transition-transform duration-200 hover:-translate-y-1">
      {/* Top-Right Bauhaus Geometric Accent */}
      <div className="absolute top-4 right-4 flex items-center space-x-1" aria-hidden="true">
        <div className="w-3.5 h-3.5 rounded-full bg-[#D02020] border border-[#121212]" />
        <div className="w-3.5 h-3.5 rounded-none bg-[#1040C0] border border-[#121212]" />
        <div className="w-3.5 h-3.5 bg-[#F0C020] border border-[#121212]" style={{ clipPath: 'polygon(50% 0%, 0% 100%, 100% 100%)' }} />
      </div>

      <div className="space-y-6">
        {/* Header: Certificate & Decision */}
        <div className="flex flex-col sm:flex-row sm:items-center justify-between pb-4 border-b-2 border-[#121212] gap-3">
          <div className="flex items-center space-x-3">
            <div className="w-10 h-10 rounded-none bg-[#1040C0] text-white border-2 border-[#121212] shadow-[2px_2px_0px_0px_#121212] flex items-center justify-center">
              <Fingerprint className="w-5 h-5" />
            </div>
            <div>
              <h3 className="text-base font-black uppercase tracking-tight text-[#121212]">
                Cryptographic Evidence & Provenance
              </h3>
              <p className="text-xs font-mono font-bold text-[#121212]/70 mt-0.5">
                Certificate ID: <span className="text-[#121212]">{certificate.certificate_id}</span>
              </p>
            </div>
          </div>

          <div className="flex items-center space-x-2.5">
            <span className={`text-xs font-mono font-black uppercase px-3 py-1 rounded-none border-2 border-[#121212] shadow-[2px_2px_0px_0px_#121212] ${
              isHighRisk ? 'bg-[#D02020] text-white' : 'bg-[#F0C020] text-[#121212]'
            }`}>
              {certificate.overall_decision.replace(/_/g, ' ')}
            </span>
            <span className="text-xs text-[#121212] font-mono font-black flex items-center gap-1.5 px-2.5 py-1 rounded-none bg-[#E0E0E0] border-2 border-[#121212] tabular">
              <Clock className="w-3.5 h-3.5 text-[#121212]" />
              {certificate.execution_latency_ms.toFixed(1)}ms
            </span>
          </div>
        </div>

        {/* Minimal SHA-256 Hash Box with Inset Top-Right Copy Button */}
        <div className="bg-[#F0F0F0] border-2 lg:border-4 border-[#121212] rounded-none p-4 space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-xs font-mono font-black text-[#121212] uppercase tracking-wider">
              SHA-256 Canonical Forensic Checksum
            </span>
            <button
              onClick={handleCopyDigest}
              className="px-3 py-1 rounded-none text-xs font-black uppercase text-white hover:bg-[#D02020] bg-[#121212] border-2 border-[#121212] shadow-[2px_2px_0px_0px_#121212] active:translate-x-[1px] active:translate-y-[1px] active:shadow-none flex items-center gap-1.5 cursor-pointer transition-colors"
            >
              {copied ? (
                <>
                  <Check className="w-3.5 h-3.5 text-[#F0C020]" />
                  <span>Copied</span>
                </>
              ) : (
                <>
                  <Copy className="w-3.5 h-3.5 text-white" />
                  <span>Copy</span>
                </>
              )}
            </button>
          </div>
          <div className="font-mono text-xs sm:text-sm font-bold text-[#121212] break-all select-all tracking-wide bg-white p-2.5 border-2 border-[#121212]">
            {certificate.cryptographic_digest}
          </div>
        </div>

        {/* Forensic Conclusion */}
        <div className="space-y-1.5">
          <span className="text-xs font-mono uppercase tracking-wider text-[#121212] font-black block">
            Autonomous Graph Reasoning Conclusion
          </span>
          <p className="text-sm text-[#121212] leading-relaxed font-medium bg-[#FFF9C4] p-4 rounded-none border-2 border-[#121212] border-l-4 border-l-[#D02020]">
            {certificate.forensic_conclusion}
          </p>
        </div>

        {/* Stable Ordered List (OL) of Traversal Hops */}
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <span className="text-xs font-mono uppercase tracking-wider text-[#121212] font-black">
              Topological Traversal Hops ({certificate.topological_evidence.length})
            </span>
            <span className="text-xs font-mono text-[#121212]">
              Confidence: <span className="font-black bg-[#F0C020] px-1.5 py-0.5 border border-[#121212]">{Math.round(confidence * 100)}%</span>
            </span>
          </div>

          <ol className="space-y-2.5 list-none p-0 m-0">
            {certificate.topological_evidence.map((step) => {
              const isExpanded = expandedStep === step.step_number;
              return (
                <li key={step.step_number} className="border-2 border-[#121212] shadow-[3px_3px_0px_0px_#121212] rounded-none bg-white overflow-hidden transition-all">
                  <div
                    onClick={() => setExpandedStep(isExpanded ? null : step.step_number)}
                    className="p-3.5 flex items-start justify-between gap-3 cursor-pointer hover:bg-[#F0F0F0] transition-colors"
                  >
                    <div className="flex items-start gap-3">
                      <span className="w-7 h-7 rounded-none bg-[#121212] text-white font-mono text-xs flex items-center justify-center font-black mt-0.5 shrink-0 border border-[#121212]">
                        {step.step_number}
                      </span>
                      <div className="space-y-0.5">
                        <div className="text-sm font-black uppercase text-[#121212] tracking-tight">{step.finding}</div>
                        <div className="text-xs text-[#121212]/80 font-mono">{step.traversal_expression}</div>
                      </div>
                    </div>
                    <div className="flex items-center gap-2 shrink-0 mt-0.5">
                      <span className="text-xs font-mono text-white font-black px-2 py-0.5 rounded-none bg-[#D02020] border-2 border-[#121212]">
                        +{step.risk_delta} Risk
                      </span>
                      {isExpanded ? <ChevronUp className="w-4 h-4 text-[#121212]" /> : <ChevronDown className="w-4 h-4 text-[#121212]" />}
                    </div>
                  </div>

                  {isExpanded && (
                    <div className="px-4 pb-4 pt-2 border-t-2 border-[#121212] bg-[#121212] space-y-1.5">
                      <div className="flex items-center justify-between text-xs text-white font-mono">
                        <span className="flex items-center gap-1.5 text-white font-bold uppercase">
                          <Terminal className="w-3.5 h-3.5 text-[#F0C020]" />
                          Verified openCypher Query
                        </span>
                        <span className="text-[#F0C020] font-black">FalkorDB Graph</span>
                      </div>
                      <pre className="text-xs font-mono text-white bg-black p-3 rounded-none border border-zinc-700 overflow-x-auto leading-relaxed">
                        <code>{step.cypher_source}</code>
                      </pre>
                    </div>
                  )}
                </li>
              );
            })}
          </ol>
        </div>

      </div>
    </div>
  );
}
