import { Line, LineChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import type { OpsPoint } from "../types";

function TooltipContent({ active, payload }: { active?: boolean; payload?: { value: number }[] }) {
  if (!active || !payload?.length) return null;
  return (
    <div className="tooltip-box">
      <div className="tooltip-label">ops/sec</div>
      <div>{payload[0].value.toFixed(1)}</div>
    </div>
  );
}

export function OpsChart({ data }: { data: OpsPoint[] }) {
  if (data.length < 2) {
    return <div className="empty-state">Waiting for traffic…</div>;
  }
  return (
    <ResponsiveContainer width="100%" height={220}>
      <LineChart data={data} margin={{ top: 4, right: 8, bottom: 0, left: 0 }}>
        <XAxis dataKey="t" hide />
        <YAxis
          width={40}
          tick={{ fill: "var(--text-muted)", fontSize: 11 }}
          axisLine={{ stroke: "var(--baseline)" }}
          tickLine={false}
          allowDecimals={false}
        />
        <Tooltip content={<TooltipContent />} cursor={{ stroke: "var(--baseline)", strokeWidth: 1 }} />
        <Line
          type="monotone"
          dataKey="opsPerSec"
          stroke="var(--series-1)"
          strokeWidth={2}
          dot={false}
          isAnimationActive={false}
        />
      </LineChart>
    </ResponsiveContainer>
  );
}
