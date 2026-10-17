import { SubgraphPayload, GraphNode, GraphEdge } from '@/lib/api';

export interface SimNode extends GraphNode {
  x: number;
  y: number;
  vx: number;
  vy: number;
  radius: number;
  fillColor: string;
  borderColor: string;
  textColor: string;
  subtitle: string;
}

export interface SimEdge extends GraphEdge {
  sourceNode?: SimNode;
  targetNode?: SimNode;
}

// Pure Bauhaus Constructivist Palette (Red #D02020, Blue #1040C0, Yellow #F0C020, Black #121212, White #FFFFFF)
export const BAUHAUS_GROUP_THEMES: Record<string, { fill: string; border: string; text: string; label: string }> = {
  target_claim: { fill: '#D02020', border: '#121212', text: '#FFFFFF', label: 'TARGET CLAIM' },
  linked_claim: { fill: '#F0C020', border: '#121212', text: '#121212', label: 'PRIOR CLAIM' },
  accident: { fill: '#1040C0', border: '#121212', text: '#FFFFFF', label: 'ACCIDENT' },
  location: { fill: '#E0E0E0', border: '#121212', text: '#121212', label: 'LOCATION' },
  repair_shop: { fill: '#FFFFFF', border: '#121212', text: '#121212', label: 'REPAIR FACILITY' },
  customer: { fill: '#F0F0F0', border: '#121212', text: '#121212', label: 'CLAIMANT' },
  vehicle: { fill: '#121212', border: '#121212', text: '#FFFFFF', label: 'VEHICLE' },
};

export function initializeSimGraph(
  subgraph: SubgraphPayload,
  selectedClaimId: string,
  width: number,
  height: number,
): { nodes: SimNode[]; edges: SimEdge[] } {
  if (!subgraph || !subgraph.nodes || subgraph.nodes.length === 0) {
    return { nodes: [], edges: [] };
  }

  const centerX = width / 2;
  const centerY = height / 2;
  const count = subgraph.nodes.length;
  const spread = Math.min(width, height) * 0.36;

  const simNodes: SimNode[] = subgraph.nodes.map((node, i) => {
    const angle = (i / count) * 2 * Math.PI - Math.PI / 2;
    const theme = BAUHAUS_GROUP_THEMES[node.group] || BAUHAUS_GROUP_THEMES.location;
    const isTarget = node.id === selectedClaimId || node.group === 'target_claim';
    const radius = isTarget ? 30 : node.group === 'repair_shop' ? 24 : 20;

    return {
      ...node,
      x: centerX + Math.cos(angle) * (spread + (i % 2 === 0 ? 30 : -20)),
      y: centerY + Math.sin(angle) * (spread + (i % 2 === 0 ? 30 : -20)),
      vx: 0,
      vy: 0,
      radius,
      fillColor: theme.fill,
      borderColor: theme.border,
      textColor: theme.text,
      subtitle: theme.label,
    };
  });

  const nodeMap = new Map<string, SimNode>();
  simNodes.forEach((n) => nodeMap.set(n.id, n));

  const simEdges: SimEdge[] = subgraph.edges.map((e) => ({
    ...e,
    sourceNode: nodeMap.get(e.from),
    targetNode: nodeMap.get(e.to),
  }));

  return { nodes: simNodes, edges: simEdges };
}

export function stepGraphPhysics(nodes: SimNode[], edges: SimEdge[], draggingNode: SimNode | null, width: number, height: number) {
  edges.forEach((edge) => {
    if (!edge.sourceNode || !edge.targetNode) return;
    const dx = edge.targetNode.x - edge.sourceNode.x;
    const dy = edge.targetNode.y - edge.sourceNode.y;
    const dist = Math.sqrt(dx * dx + dy * dy) || 1;
    const targetDist = 160;
    const force = (dist - targetDist) * 0.003;
    const fx = (dx / dist) * force;
    const fy = (dy / dist) * force;

    if (edge.sourceNode !== draggingNode) {
      edge.sourceNode.vx += fx;
      edge.sourceNode.vy += fy;
    }
    if (edge.targetNode !== draggingNode) {
      edge.targetNode.vx -= fx;
      edge.targetNode.vy -= fy;
    }
  });

  for (let i = 0; i < nodes.length; i++) {
    for (let j = i + 1; j < nodes.length; j++) {
      const n1 = nodes[i];
      const n2 = nodes[j];
      const dx = n2.x - n1.x;
      const dy = n2.y - n1.y;
      const dist = Math.sqrt(dx * dx + dy * dy) || 1;
      if (dist < 280) {
        const rep = 900 / (dist * dist);
        const rx = (dx / dist) * rep;
        const ry = (dy / dist) * rep;
        if (n1 !== draggingNode) {
          n1.vx -= rx;
          n1.vy -= ry;
        }
        if (n2 !== draggingNode) {
          n2.vx += rx;
          n2.vy += ry;
        }
      }
    }
  }

  const centerX = width / 2;
  const centerY = height / 2;
  nodes.forEach((n) => {
    if (n === draggingNode) return;
    n.vx += (centerX - n.x) * 0.0005;
    n.vy += (centerY - n.y) * 0.0005;
    n.vx *= 0.85;
    n.vy *= 0.85;
    n.x += n.vx;
    n.y += n.vy;
  });
}
