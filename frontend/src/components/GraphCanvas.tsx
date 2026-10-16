'use client';

import React, { useRef, useEffect } from 'react';
import { SubgraphPayload } from '@/lib/api';
import { SimNode, SimEdge, initializeSimGraph, stepGraphPhysics } from '@/lib/graphPhysics';
import { renderBauhausGraph } from '@/lib/graphRenderer';
import { GraphCanvasHud } from '@/components/GraphCanvasHud';
import { useCanvasPanZoom } from '@/lib/useCanvasPanZoom';

interface GraphCanvasProps {
  subgraph: SubgraphPayload;
  selectedClaimId: string;
  isInvestigating: boolean;
  /** Node ids on the evidence path to highlight; others are dimmed. */
  evidenceIds?: Set<string> | null;
}

export function GraphCanvas({ subgraph, selectedClaimId, isInvestigating, evidenceIds = null }: GraphCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const nodesRef = useRef<SimNode[]>([]);
  const edgesRef = useRef<SimEdge[]>([]);

  const {
    transformRef,
    scaleDisplay,
    draggingNodeRef,
    hoveredNode,
    tooltipPos,
    handleMouseDown,
    handleMouseMove,
    handleMouseUp,
    handleWheel,
    handleZoom,
    handleResetZoom,
  } = useCanvasPanZoom(nodesRef, canvasRef);

  useEffect(() => {
    const width = containerRef.current?.clientWidth || 800;
    const height = containerRef.current?.clientHeight || 540;
    const { nodes, edges } = initializeSimGraph(subgraph, selectedClaimId, width, height);
    nodesRef.current = nodes;
    edgesRef.current = edges;
    transformRef.current = { x: 0, y: 0, scale: 1 };
  }, [subgraph, selectedClaimId, transformRef]);

  useEffect(() => {
    const handleResize = () => {
      if (!canvasRef.current || !containerRef.current) return;
      const dpr = window.devicePixelRatio || 1;
      const rect = containerRef.current.getBoundingClientRect();
      canvasRef.current.width = rect.width * dpr;
      canvasRef.current.height = rect.height * dpr;
      const ctx = canvasRef.current.getContext('2d');
      if (ctx) ctx.scale(dpr, dpr);
    };
    handleResize();
    window.addEventListener('resize', handleResize);
    return () => window.removeEventListener('resize', handleResize);
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    let running = true;
    let iteration = 0;

    const render = () => {
      if (!running) return;
      const dpr = window.devicePixelRatio || 1;
      const width = canvas.width / dpr;
      const height = canvas.height / dpr;

      if (iteration < 180 || draggingNodeRef.current) {
        iteration++;
        stepGraphPhysics(nodesRef.current, edgesRef.current, draggingNodeRef.current, width, height);
      }

      renderBauhausGraph(
        ctx,
        width,
        height,
        nodesRef.current,
        edgesRef.current,
        transformRef.current,
        hoveredNode,
        selectedClaimId,
        evidenceIds
      );

      requestAnimationFrame(render);
    };

    const animId = requestAnimationFrame(render);
    return () => {
      running = false;
      cancelAnimationFrame(animId);
    };
  }, [hoveredNode, selectedClaimId, evidenceIds, transformRef, draggingNodeRef]);

  return (
    <div ref={containerRef} className="relative w-full h-[540px] bg-[#F0F0F0] border-2 lg:border-4 border-[#121212] shadow-[6px_6px_0px_0px_#121212] lg:shadow-[8px_8px_0px_0px_#121212] rounded-none overflow-hidden select-none">
      <GraphCanvasHud
        vertexCount={subgraph.nodes.length}
        edgeCount={subgraph.edges.length}
        isInvestigating={isInvestigating}
        scaleDisplay={scaleDisplay}
        onZoomIn={() => handleZoom(1.2)}
        onZoomOut={() => handleZoom(0.8)}
        onResetZoom={handleResetZoom}
        hoveredNode={hoveredNode}
        tooltipPos={tooltipPos}
      />
      <canvas
        ref={canvasRef}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseUp}
        onWheel={handleWheel}
        className="w-full h-full cursor-grab active:cursor-grabbing block"
      />
    </div>
  );
}
