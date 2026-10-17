'use client';

import React from 'react';
import { RotateCw } from 'lucide-react';

interface NavbarProps {
  engineOnline: boolean;
  onReseed: () => void;
  isReseeding: boolean;
  seedEnabled: boolean;
}

export function Navbar({
  engineOnline,
  onReseed,
  isReseeding,
  seedEnabled,
}: NavbarProps) {
  return (
    <header className="sticky top-0 z-50 w-full bg-[#F0F0F0] border-b-4 border-[#121212] select-none">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 h-16 flex items-center justify-between">
        
        {/* Brand: Bauhaus Constructivist Geometric Identity */}
        <div className="flex items-center space-x-3">
          <div className="flex items-center space-x-1.5" aria-label="Bauhaus Logo">
            {/* Bauhaus Red Circle */}
            <div className="w-6 h-6 rounded-full bg-[#D02020] border-2 border-[#121212] shadow-[2px_2px_0px_0px_#121212]" />
            {/* Bauhaus Blue Square */}
            <div className="w-6 h-6 rounded-none bg-[#1040C0] border-2 border-[#121212] shadow-[2px_2px_0px_0px_#121212]" />
            {/* Bauhaus Yellow Triangle */}
            <div 
              className="w-6 h-6 bg-[#F0C020] border-2 border-[#121212] shadow-[2px_2px_0px_0px_#121212]" 
              style={{ clipPath: 'polygon(50% 0%, 0% 100%, 100% 100%)' }} 
            />
          </div>
          <div className="flex items-baseline space-x-2">
            <span className="text-xl font-black uppercase tracking-tighter text-[#121212]">
              Diktiax
            </span>
            <span className="hidden sm:inline-block text-[11px] font-bold uppercase tracking-widest text-[#121212]/70 border-l-2 border-[#121212] pl-2">
              Graph Reasoning Engine
            </span>
          </div>
        </div>

        {/* Right Status Badge & Mechanical Reseed Action */}
        <div className="flex items-center space-x-3 text-xs">
          <div className={`flex items-center space-x-2 px-3 py-1 font-bold uppercase tracking-wider border-2 border-[#121212] shadow-[2px_2px_0px_0px_#121212] ${
            engineOnline ? 'bg-[#F0C020] text-[#121212]' : 'bg-[#E0E0E0] text-[#121212]'
          }`}>
            <span className={`w-2.5 h-2.5 rounded-full border border-[#121212] ${
              engineOnline ? 'bg-[#16A34A] animate-pulse' : 'bg-[#121212]'
            }`} />
            <span>{engineOnline ? 'FalkorDB Active' : 'Backend Offline'}</span>
          </div>

          {seedEnabled && (
          <button
            onClick={onReseed}
            disabled={isReseeding}
            className="p-1.5 bg-white text-[#121212] border-2 border-[#121212] shadow-[3px_3px_0px_0px_#121212] active:translate-x-[2px] active:translate-y-[2px] active:shadow-none hover:bg-[#F0C020] transition-colors cursor-pointer disabled:opacity-50"
            title="Reseed Graph Database"
            aria-label="Reseed graph database"
          >
            <RotateCw className={`w-4 h-4 ${isReseeding ? 'animate-spin' : ''}`} />
          </button>
          )}
        </div>

      </div>
    </header>
  );
}
