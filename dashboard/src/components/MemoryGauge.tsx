function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

/** Rounds up to a friendly ceiling so the gauge has headroom instead of
 * constantly redefining its own scale as memory grows. */
function niceCeiling(bytes: number): number {
  const steps = [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024].map((mb) => mb * 1024 * 1024);
  for (const step of steps) {
    if (bytes <= step * 0.8) return step;
  }
  // Beyond 1GB: round up to the next 512MB.
  const half = 512 * 1024 * 1024;
  return Math.ceil(bytes / half) * half;
}

const SIZE = 180;
const STROKE = 16;
const RADIUS = (SIZE - STROKE) / 2;
const CENTER = SIZE / 2;
// Semicircle gauge: sweeps 180deg from the left to the right.
const CIRCUMFERENCE = Math.PI * RADIUS;

export function MemoryGauge({ bytes, liveBytes }: { bytes: number; liveBytes: number }) {
  const max = niceCeiling(bytes);
  const fraction = max === 0 ? 0 : Math.min(bytes / max, 1);
  const offset = CIRCUMFERENCE * (1 - fraction);

  return (
    <div className="gauge-wrap">
      <svg width={SIZE} height={SIZE / 2 + STROKE} viewBox={`0 0 ${SIZE} ${SIZE / 2 + STROKE}`}>
        <path
          d={`M ${STROKE / 2} ${CENTER} A ${RADIUS} ${RADIUS} 0 0 1 ${SIZE - STROKE / 2} ${CENTER}`}
          fill="none"
          stroke="var(--gridline)"
          strokeWidth={STROKE}
          strokeLinecap="round"
        />
        <path
          d={`M ${STROKE / 2} ${CENTER} A ${RADIUS} ${RADIUS} 0 0 1 ${SIZE - STROKE / 2} ${CENTER}`}
          fill="none"
          stroke="var(--series-1)"
          strokeWidth={STROKE}
          strokeLinecap="round"
          strokeDasharray={CIRCUMFERENCE}
          strokeDashoffset={offset}
          style={{ transition: "stroke-dashoffset 400ms ease" }}
        />
      </svg>
      <div className="gauge-value">{formatBytes(bytes)}</div>
      <div className="gauge-sub">
        {formatBytes(liveBytes)} live &middot; ceiling {formatBytes(max)}
      </div>
    </div>
  );
}
