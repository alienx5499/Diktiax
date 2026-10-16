import { useRef, useState, useCallback } from 'react';
import { SimNode } from './graphPhysics';

export function useCanvasPanZoom(nodesRef: React.MutableRefObject<SimNode[]>, canvasRef: React.RefObject<HTMLCanvasElement | null>) {
  const transformRef = useRef({ x: 0, y: 0, scale: 1 });
  const [scaleDisplay, setScaleDisplay] = useState(1);
  const draggingNodeRef = useRef<SimNode | null>(null);
  const isPanningRef = useRef(false);
  const lastMousePosRef = useRef({ x: 0, y: 0 });
  const [hoveredNode, setHoveredNode] = useState<SimNode | null>(null);
  const [tooltipPos, setTooltipPos] = useState({ x: 0, y: 0 });

  const screenToWorld = useCallback((screenX: number, screenY: number) => {
    const { x, y, scale } = transformRef.current;
    return { x: (screenX - x) / scale, y: (screenY - y) / scale };
  }, []);

  const handleMouseDown = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const rect = canvasRef.current?.getBoundingClientRect();
    if (!rect) return;
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;
    const worldPos = screenToWorld(mouseX, mouseY);

    const clicked = nodesRef.current.find((n) => {
      const dx = n.x - worldPos.x;
      const dy = n.y - worldPos.y;
      return Math.sqrt(dx * dx + dy * dy) <= n.radius + 8;
    });

    if (clicked) {
      draggingNodeRef.current = clicked;
    } else {
      isPanningRef.current = true;
      lastMousePosRef.current = { x: e.clientX, y: e.clientY };
    }
  };

  const handleMouseMove = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const rect = canvasRef.current?.getBoundingClientRect();
    if (!rect) return;
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;

    if (draggingNodeRef.current) {
      const worldPos = screenToWorld(mouseX, mouseY);
      draggingNodeRef.current.x = worldPos.x;
      draggingNodeRef.current.y = worldPos.y;
      draggingNodeRef.current.vx = 0;
      draggingNodeRef.current.vy = 0;
      return;
    }

    if (isPanningRef.current) {
      const dx = e.clientX - lastMousePosRef.current.x;
      const dy = e.clientY - lastMousePosRef.current.y;
      transformRef.current.x += dx;
      transformRef.current.y += dy;
      lastMousePosRef.current = { x: e.clientX, y: e.clientY };
      return;
    }

    const worldPos = screenToWorld(mouseX, mouseY);
    const found = nodesRef.current.find((n) => {
      const dx = n.x - worldPos.x;
      const dy = n.y - worldPos.y;
      return Math.sqrt(dx * dx + dy * dy) <= n.radius + 8;
    });

    if (found) {
      setHoveredNode(found);
      setTooltipPos({ x: mouseX + 18, y: mouseY + 18 });
    } else {
      setHoveredNode(null);
    }
  };

  const handleMouseUp = () => {
    draggingNodeRef.current = null;
    isPanningRef.current = false;
  };

  const handleWheel = (e: React.WheelEvent<HTMLCanvasElement>) => {
    e.preventDefault();
    const zoomFactor = e.deltaY < 0 ? 1.08 : 0.92;
    const rect = canvasRef.current?.getBoundingClientRect();
    if (!rect) return;
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;
    const currentScale = transformRef.current.scale;
    const newScale = Math.min(Math.max(currentScale * zoomFactor, 0.4), 2.5);
    transformRef.current.x = mouseX - (mouseX - transformRef.current.x) * (newScale / currentScale);
    transformRef.current.y = mouseY - (mouseY - transformRef.current.y) * (newScale / currentScale);
    transformRef.current.scale = newScale;
    setScaleDisplay(newScale);
  };

  const handleZoom = (factor: number) => {
    const s = Math.min(Math.max(transformRef.current.scale * factor, 0.4), 2.5);
    transformRef.current.scale = s;
    setScaleDisplay(s);
  };

  const handleResetZoom = () => {
    transformRef.current = { x: 0, y: 0, scale: 1 };
    setScaleDisplay(1);
  };

  return {
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
  };
}
