import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import type { TopKey } from "../types";

function TooltipContent({ active, payload }: { active?: boolean; payload?: { payload: TopKey }[] }) {
  if (!active || !payload?.length) return null;
  const { key, count } = payload[0].payload;
  return (
    <div className="tooltip-box">
      <div className="tooltip-label">{key}</div>
      <div>{count} accesses</div>
    </div>
  );
}

export function TopKeysChart({ data }: { data: TopKey[] }) {
  if (data.length === 0) {
    return <div className="empty-state">No key accesses yet</div>;
  }
  const sorted = [...data].sort((a, b) => a.count - b.count).slice(-10);
  return (
    <ResponsiveContainer width="100%" height={260}>
      <BarChart data={sorted} layout="vertical" margin={{ top: 4, right: 16, bottom: 4, left: 4 }}>
        <CartesianGrid horizontal={false} stroke="var(--gridline)" />
        <XAxis type="number" allowDecimals={false} tick={{ fill: "var(--text-muted)", fontSize: 11 }} axisLine={{ stroke: "var(--baseline)" }} tickLine={false} />
        <YAxis
          type="category"
          dataKey="key"
          width={90}
          tick={{ fill: "var(--text-secondary)", fontSize: 12 }}
          axisLine={{ stroke: "var(--baseline)" }}
          tickLine={false}
        />
        <Tooltip content={<TooltipContent />} cursor={{ fill: "rgba(255,255,255,0.04)" }} />
        <Bar dataKey="count" fill="var(--series-1)" radius={[0, 4, 4, 0]} isAnimationActive={false} barSize={16} />
      </BarChart>
    </ResponsiveContainer>
  );
}
