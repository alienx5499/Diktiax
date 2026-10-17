'use client';

import React from 'react';
import { FraudPatternMatch } from '@/lib/api';

interface PatternFilterProps {
  patterns: FraudPatternMatch[];
  active: string | null;
  onChange: (name: string | null) => void;
}

/** Chips that focus the graph on one pattern's evidence path (none selected = all patterns). */
export function PatternFilter({ patterns, active, onChange }: PatternFilterProps) {
  if (patterns.length === 0) return null;
  const chip =
    'px-3 py-1.5 font-mono text-xs font-black uppercase border-2 border-[#121212] shadow-[2px_2px_0px_0px_#121212] cursor-pointer rounded-none focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#1040C0]';
  return (
    <div className="flex flex-wrap items-center gap-2" role="group" aria-label="Highlight fraud pattern evidence path">
      <span className="text-[11px] font-bold uppercase tracking-widest text-[#121212]/70">Evidence path</span>
      <button
        type="button"
        aria-pressed={active === null}
        onClick={() => onChange(null)}
        className={`${chip} ${active === null ? 'bg-[#D02020] text-white' : 'bg-white hover:bg-[#F0C020]'}`}
      >
        All patterns
      </button>
      {patterns.map((p) => (
        <button
          key={p.pattern_name}
          type="button"
          aria-pressed={active === p.pattern_name}
          onClick={() => onChange(active === p.pattern_name ? null : p.pattern_name)}
          className={`${chip} ${active === p.pattern_name ? 'bg-[#D02020] text-white' : 'bg-white hover:bg-[#F0C020]'}`}
        >
          {p.pattern_name}
        </button>
      ))}
    </div>
  );
}
