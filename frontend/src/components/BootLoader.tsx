'use client';
import React, { useEffect, useRef } from 'react';

// Rose-orbit curve r(t) = R - A·s·cos(k·t) (after paidax01/math-curve-loaders), drawn as a
// comet of particles. Shown on first load while a free-tier backend wakes up.
const PARTICLES = 64;
const TRAIL = 0.42;
const LAP_MS = 5200;
const SPIN_MS = 28000;
const PULSE_MS = 4600;
const R = 7;
const A = 2.7;
const K = 7;
const SCALE = 3.9;
// The curve in Bauhaus thirds: red, blue, yellow. Particles take the colour of their third.
const THIRDS = ['#D02020', '#1040C0', '#F0C020'];
const colorAt = (progress: number) => THIRDS[Math.min(2, Math.floor(progress * 3))];

function point(progress: number, detail: number) {
  const t = progress * Math.PI * 2;
  const r = R - A * detail * Math.cos(K * t);
  return { x: 50 + Math.cos(t) * r * SCALE, y: 50 + Math.sin(t) * r * SCALE };
}

function curvePath(detail: number, from = 0, to = 1, steps = 120) {
  let d = '';
  for (let i = 0; i <= steps; i++) {
    const p = point(from + ((to - from) * i) / steps, detail);
    d += `${i === 0 ? 'M' : 'L'}${p.x.toFixed(2)} ${p.y.toFixed(2)}`;
  }
  return d;
}

export function BootLoader() {
  const group = useRef<SVGGElement>(null);
  const paths = useRef<(SVGPathElement | null)[]>([]);
  const dots = useRef<(SVGCircleElement | null)[]>([]);

  useEffect(() => {
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      paths.current.forEach((p, i) => p?.setAttribute('d', curvePath(0.75, i / 3, (i + 1) / 3)));
      return;
    }
    const start = performance.now();
    let frame = 0;
    const render = (now: number) => {
      const time = now - start;
      const lap = (time % LAP_MS) / LAP_MS;
      const detail = 0.52 + ((Math.sin(((time % PULSE_MS) / PULSE_MS) * Math.PI * 2 + 0.55) + 1) / 2) * 0.48;
      group.current?.setAttribute('transform', `rotate(${-((time % SPIN_MS) / SPIN_MS) * 360} 50 50)`);
      paths.current.forEach((p, i) => p?.setAttribute('d', curvePath(detail, i / 3, (i + 1) / 3)));
      dots.current.forEach((dot, i) => {
        if (!dot) return;
        const tail = i / (PARTICLES - 1);
        const progress = (((lap - tail * TRAIL) % 1) + 1) % 1;
        const p = point(progress, detail);
        const fade = Math.pow(1 - tail, 0.56);
        dot.setAttribute('fill', colorAt(progress));
        dot.setAttribute('cx', p.x.toFixed(2));
        dot.setAttribute('cy', p.y.toFixed(2));
        dot.setAttribute('r', (0.9 + fade * 2.7).toFixed(2));
        dot.setAttribute('opacity', (0.04 + fade * 0.96).toFixed(3));
      });
      frame = requestAnimationFrame(render);
    };
    frame = requestAnimationFrame(render);
    return () => cancelAnimationFrame(frame);
  }, []);

  return (
    <div
      role="status"
      aria-live="polite"
      className="fixed inset-0 z-[60] flex flex-col items-center justify-center gap-8 bg-[#F0F0F0] px-6 text-center"
    >
      <svg viewBox="0 0 100 100" fill="none" aria-hidden="true" className="w-56 h-56 sm:w-72 sm:h-72 overflow-visible">
        <g ref={group}>
          {THIRDS.map((color, i) => (
            <path
              key={color}
              ref={(el) => {
                paths.current[i] = el;
              }}
              stroke={color}
              strokeWidth={5.2}
              strokeLinecap="round"
              strokeLinejoin="round"
              opacity={0.22}
            />
          ))}
          {Array.from({ length: PARTICLES }, (_, i) => (
            <circle
              key={i}
              ref={(el) => {
                dots.current[i] = el;
              }}
            />
          ))}
        </g>
      </svg>
      <div className="space-y-2">
        <p className="text-2xl sm:text-3xl font-black uppercase tracking-tighter text-[#121212]">Waking up the graph engine</p>
        <p className="text-xs font-bold uppercase tracking-widest text-[#121212]/70">
          FalkorDB and the agent run on a free tier and can take up to a minute to start
        </p>
      </div>
    </div>
  );
}
