'use client';

import React from 'react';

export function Footer() {
  return (
    <footer className="border-t-4 border-[#121212] bg-[#121212] py-6 text-xs text-[#F0F0F0] select-none mt-12">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 flex flex-col sm:flex-row items-center justify-between gap-4">
        {/* Left: Bauhaus geometric brand mark */}
        <div className="flex items-center space-x-3">
          <div className="flex items-center space-x-1.5" aria-hidden="true">
            <span className="w-3 h-3 rounded-full bg-[#D02020]" />
            <span className="w-3 h-3 rounded-none bg-[#1040C0]" />
            <span className="w-3 h-3 bg-[#F0C020]" style={{ clipPath: 'polygon(50% 0%, 0% 100%, 100% 100%)' }} />
          </div>
          <span className="text-white font-black uppercase tracking-wider">Diktiax Platform</span>
          <span className="text-zinc-600">•</span>
          <span className="text-zinc-400 uppercase font-mono">Deterministic Graph Reasoning Engine</span>
        </div>

        {/* Right: Security & Architecture spec */}
        <div className="text-zinc-400 font-mono text-[11px] uppercase tracking-wider">
          FalkorDB Subgraph Architecture • SHA-256 Checkable Provenance
        </div>
      </div>
    </footer>
  );
}
