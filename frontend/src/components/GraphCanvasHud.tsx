'use client';

import React from 'react';
import { ZoomIn, ZoomOut, Maximize2, Layers, Sparkles } from 'lucide-react';
import { SimNode } from '@/lib/graphPhysics';

interface GraphCanvasHudProps {
  vertexCount: number;
  edgeCount: number;
  isInvestigating: boolean;
  scaleDisplay: number;
  onZoomIn: () => void;
  onZoomOut: () => void;
  onResetZoom: () => void;
  hoveredNode: SimNode | null;
  tooltipPos: { x: number; y: number };
}

export function GraphCanvasHud({
  vertexCount,
  edgeCount,
  isInvestigating,
  scaleDisplay,
  onZoomIn,
  onZoomOut,
  onResetZoom,
  hoveredNode,
  tooltipPos,
}: GraphCanvasHudProps) {
  return (
    <>
      {/* Top Header Overlay Badges */}
      <div className="absolute top-4 left-4 z-10 flex flex-wrap items-center gap-2">
        <div className="px-3.5 py-1.5 bg-white border-2 border-[#121212] shadow-[3px_3px_0px_0px_#121212] rounded-none text-xs font-black uppercase text-[#121212] flex items-center space-x-2">
          <Layers className="w-4 h-4 text-[#1040C0]" />
          <span>FalkorDB Subgraph Canvas</span>
          <span className="text-[#121212]/40">•</span>
          <span className="bg-[#F0C020] px-1.5 py-0.5 border border-[#121212]">{vertexCount} Vertices</span>
          <span className="bg-[#E0E0E0] px-1.5 py-0.5 border border-[#121212]">{edgeCount} Edges</span>
        </div>
        {isInvestigating && (
          <div className="px-3.5 py-1.5 bg-[#D02020] text-white border-2 border-[#121212] shadow-[3px_3px_0px_0px_#121212] rounded-none text-xs font-black uppercase flex items-center space-x-2 animate-pulse">
            <Sparkles className="w-4 h-4 text-[#F0C020]" />
            <span>Traversing Multi-Hop Cycle...</span>
          </div>
        )}
      </div>

      {/* Navigation HUD Controls */}
      <div className="absolute bottom-4 left-1/2 -translate-x-1/2 z-10 px-3 py-1.5 bg-white border-2 border-[#121212] shadow-[4px_4px_0px_0px_#121212] rounded-none flex items-center space-x-2 text-xs font-bold uppercase">
        <button
          onClick={onZoomIn}
          className="p-1.5 bg-[#F0F0F0] hover:bg-[#F0C020] border border-[#121212] active:translate-x-[1px] active:translate-y-[1px] rounded-none cursor-pointer"
          title="Zoom In"
        >
          <ZoomIn className="w-4 h-4" />
        </button>
        <button
          onClick={onResetZoom}
          className="px-2 py-1 font-mono font-black tabular hover:bg-[#E0E0E0] border border-[#121212] rounded-none cursor-pointer"
          title="Reset Zoom"
        >
          {Math.round(scaleDisplay * 100)}%
        </button>
        <button
          onClick={onZoomOut}
          className="p-1.5 bg-[#F0F0F0] hover:bg-[#F0C020] border border-[#121212] active:translate-x-[1px] active:translate-y-[1px] rounded-none cursor-pointer"
          title="Zoom Out"
        >
          <ZoomOut className="w-4 h-4" />
        </button>
        <div className="w-px h-4 bg-[#121212]" />
        <button
          onClick={onResetZoom}
          className="p-1.5 bg-[#F0F0F0] hover:bg-[#F0C020] border border-[#121212] active:translate-x-[1px] active:translate-y-[1px] rounded-none cursor-pointer"
          title="Center Canvas"
        >
          <Maximize2 className="w-4 h-4" />
        </button>
      </div>

      {/* Interactive Tooltip */}
      {hoveredNode && (
        <div
          style={{ left: tooltipPos.x, top: tooltipPos.y }}
          className="absolute z-30 pointer-events-none p-4 bg-white border-2 lg:border-4 border-[#121212] shadow-[6px_6px_0px_0px_#121212] rounded-none max-w-sm"
        >
          <div className="flex items-center justify-between gap-3 mb-2">
            <span className="text-sm font-black uppercase text-[#121212]">
              {hoveredNode.title || hoveredNode.label}
            </span>
            <span className="text-[10px] uppercase font-black px-2 py-0.5 bg-[#F0C020] border border-[#121212] rounded-none">
              {hoveredNode.group.replace(/_/g, ' ')}
            </span>
          </div>
          <p className="text-xs font-mono font-bold text-[#121212]/70 mb-2">ID: {hoveredNode.id}</p>
          <div className="pt-2 border-t-2 border-[#121212] space-y-1 text-xs font-mono">
            {Object.entries(hoveredNode.properties).map(([key, val]) => (
              <div key={key} className="flex justify-between gap-4 text-[#121212]">
                <span className="font-bold text-[#121212]/60 uppercase">{key}:</span>
                <span className="font-black truncate">{String(val)}</span>
              </div>
            ))}
          </div>
        </div>
      )}
    </>
  );
}
