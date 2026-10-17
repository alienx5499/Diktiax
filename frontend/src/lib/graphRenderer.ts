import { SimNode, SimEdge } from './graphPhysics';

export function renderBauhausGraph(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  nodes: SimNode[],
  edges: SimEdge[],
  transform: { x: number; y: number; scale: number },
  hoveredNode: SimNode | null,
  selectedClaimId: string,
  evidence: Set<string> | null = null,
) {
  // Dimming only when an evidence set is non-empty; ids absent from the subgraph are ignored.
  const focus = evidence !== null && evidence.size > 0;
  // 1. Off-white Bauhaus Canvas Background
  ctx.fillStyle = '#F0F0F0';
  ctx.fillRect(0, 0, width, height);

  ctx.save();
  ctx.translate(transform.x, transform.y);
  ctx.scale(transform.scale, transform.scale);

  // 2. Draw Edges
  edges.forEach((edge) => {
    if (!edge.sourceNode || !edge.targetNode) return;
    const s = edge.sourceNode;
    const t = edge.targetNode;
    const isHighlighted = hoveredNode && (hoveredNode.id === s.id || hoveredNode.id === t.id);
    const isEvidence = focus && evidence.has(s.id) && evidence.has(t.id);
    ctx.globalAlpha = focus && !isEvidence && !isHighlighted ? 0.15 : 1;

    ctx.beginPath();
    ctx.moveTo(s.x, s.y);
    ctx.lineTo(t.x, t.y);

    if (isEvidence || edge.isSuspicious) {
      ctx.strokeStyle = '#D02020';
      ctx.lineWidth = isEvidence ? 5 : isHighlighted ? 4 : 3;
    } else {
      ctx.strokeStyle = isHighlighted ? '#1040C0' : '#121212';
      ctx.lineWidth = isHighlighted ? 2.5 : 2;
    }
    ctx.stroke();

    // Edge Label Pill (Bauhaus Primary Block)
    if (isHighlighted || isEvidence || edge.isSuspicious) {
      const midX = (s.x + t.x) / 2;
      const midY = (s.y + t.y) / 2;
      ctx.font = '700 10px var(--font-outfit), sans-serif';
      const labelText = edge.label.replace(/_/g, ' ').toUpperCase();
      const textMetrics = ctx.measureText(labelText);
      const boxW = textMetrics.width + 12;
      const boxH = 18;

      // Hard Offset Shadow
      ctx.fillStyle = '#121212';
      ctx.fillRect(midX - boxW / 2 + 2, midY - boxH / 2 + 2, boxW, boxH);

      // Body & Border
      ctx.fillStyle = isEvidence || edge.isSuspicious ? '#F0C020' : '#FFFFFF';
      ctx.fillRect(midX - boxW / 2, midY - boxH / 2, boxW, boxH);
      ctx.strokeStyle = '#121212';
      ctx.lineWidth = 1.5;
      ctx.strokeRect(midX - boxW / 2, midY - boxH / 2, boxW, boxH);

      ctx.fillStyle = '#121212';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillText(labelText, midX, midY);
    }
  });
  ctx.globalAlpha = 1;

  // 3. Draw Nodes (Constructivist Circles with Hard Black Offset Shadows)
  nodes.forEach((node) => {
    const isHovered = hoveredNode?.id === node.id;
    const isTarget = node.id === selectedClaimId || node.group === 'target_claim';
    const isEvidence = focus && evidence.has(node.id);
    ctx.globalAlpha = focus && !isEvidence && !isTarget && !isHovered ? 0.2 : 1;

    // Hard Offset Shadow
    ctx.beginPath();
    ctx.arc(node.x + 4, node.y + 4, node.radius, 0, Math.PI * 2);
    ctx.fillStyle = '#121212';
    ctx.fill();

    // Main Node Circle
    ctx.beginPath();
    ctx.arc(node.x, node.y, node.radius, 0, Math.PI * 2);
    ctx.fillStyle = node.fillColor;
    ctx.fill();

    // Stark Black Border
    ctx.strokeStyle = '#121212';
    ctx.lineWidth = isTarget ? 4 : isHovered ? 3.5 : 2.5;
    ctx.stroke();

    // Evidence ring (Bauhaus red) so it reads even on red/dark fills
    if (isEvidence) {
      ctx.beginPath();
      ctx.arc(node.x, node.y, node.radius + 6, 0, Math.PI * 2);
      ctx.strokeStyle = '#D02020';
      ctx.lineWidth = 4;
      ctx.stroke();
    }

    // Node High-Contrast Label Card
    const labelText = (node.title || node.label).toUpperCase();
    ctx.font = isTarget ? '900 12px var(--font-outfit), sans-serif' : '700 11px var(--font-outfit), sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'top';

    const m = ctx.measureText(labelText);
    const cardW = Math.max(m.width + 16, 68);
    const cardH = 22;
    const cardY = node.y + node.radius + 8;

    // Hard Shadow for Label
    ctx.fillStyle = '#121212';
    ctx.fillRect(node.x - cardW / 2 + 2, cardY + 2, cardW, cardH);

    // Label Card Body & Border
    ctx.fillStyle = isTarget ? '#F0C020' : '#FFFFFF';
    ctx.fillRect(node.x - cardW / 2, cardY, cardW, cardH);
    ctx.strokeStyle = '#121212';
    ctx.lineWidth = 2;
    ctx.strokeRect(node.x - cardW / 2, cardY, cardW, cardH);

    // Text
    ctx.fillStyle = isTarget ? '#D02020' : '#121212';
    ctx.fillText(labelText, node.x, cardY + 4);
  });
  ctx.globalAlpha = 1;

  ctx.restore();
}
