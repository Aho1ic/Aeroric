import { useEffect, useRef } from "react";
import { createRecursiveHeroEffect } from "./recursive-hero-effect";
import "./recursive-hero-effect.css";

export interface RecursiveHeroCanvasProps {
  reducedMotion?: boolean;
  className?: string;
}

type RecursiveHeroEffect = ReturnType<typeof createRecursiveHeroEffect>;

interface CanvasEntry {
  canvas: HTMLCanvasElement;
  reducedMotion: boolean;
}

const mountedCanvases: CanvasEntry[] = [];
let activeEntry: CanvasEntry | null = null;
let activeEffect: RecursiveHeroEffect | null = null;

function activateTopCanvas() {
  activeEffect?.destroy();
  activeEffect = null;
  activeEntry = mountedCanvases[mountedCanvases.length - 1] ?? null;

  if (!activeEntry) return;

  activeEffect = createRecursiveHeroEffect(activeEntry.canvas, {
    reducedMotion: activeEntry.reducedMotion,
  });
}

export function RecursiveHeroCanvas({
  reducedMotion = false,
  className = "",
}: RecursiveHeroCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const entryRef = useRef<CanvasEntry | null>(null);

  useEffect(() => {
    if (!canvasRef.current) return;

    const entry = { canvas: canvasRef.current, reducedMotion };
    entryRef.current = entry;
    mountedCanvases.push(entry);
    activateTopCanvas();

    return () => {
      const index = mountedCanvases.indexOf(entry);
      if (index >= 0) mountedCanvases.splice(index, 1);
      entryRef.current = null;
      activateTopCanvas();
    };
  }, [reducedMotion]);

  useEffect(() => {
    const entry = entryRef.current;
    if (!entry) return;

    entry.reducedMotion = reducedMotion;
    if (entry === activeEntry) {
      activeEffect?.setReducedMotion(reducedMotion);
    }
  }, [reducedMotion]);

  return (
    <canvas
      ref={canvasRef}
      className={["recursive-hero-effect__canvas", className].filter(Boolean).join(" ")}
      data-recursive-hero-background="true"
      aria-hidden="true"
    />
  );
}

export default RecursiveHeroCanvas;
