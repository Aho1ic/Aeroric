export interface RecursiveHeroEffectOptions {
  reducedMotion?: boolean;
}

export interface RecursiveHeroEffectHandle {
  setReducedMotion(isReduced: boolean): void;
  destroy(): void;
}

export function createRecursiveHeroEffect(
  canvas: HTMLCanvasElement,
  options?: RecursiveHeroEffectOptions,
): RecursiveHeroEffectHandle;

export function shouldRestartRecursiveHeroLoop(input: {
  isFinalLayer: boolean;
  doneAt: number;
  currentTime: number;
  springVelocity: number;
}): boolean;
