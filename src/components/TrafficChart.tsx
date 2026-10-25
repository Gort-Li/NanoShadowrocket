import { Area, AreaChart, ResponsiveContainer, Tooltip, YAxis } from "recharts";
import { formatRate } from "../lib/format";
import type { HistoryPoint } from "../lib/types";

function ChartTooltip({
  active,
  payload,
}: {
  active?: boolean;
  payload?: Array<{ dataKey?: string | number; value?: number | string }>;
}) {
  if (!active || !payload?.length) return null;
  const find = (key: string) => Number(payload.find((p) => p.dataKey === key)?.value ?? 0);
  return (
    <div className="rounded-lg border border-hairline bg-[#171B21]/95 px-2.5 py-1.5 text-[11px] shadow-card backdrop-blur-xl">
      <p className="text-info">下载 {formatRate(find("down")).value} {formatRate(find("down")).unit}</p>
      <p className="mt-0.5 text-ok">上传 {formatRate(find("up")).value} {formatRate(find("up")).unit}</p>
    </div>
  );
}

export default function TrafficChart({
  data,
  height = 132,
  dimmed = false,
}: {
  data: HistoryPoint[];
  height?: number;
  dimmed?: boolean;
}) {
  const rows = data.map((point, index) => ({
    index,
    up: point.up,
    down: point.down,
  }));
  const peak = rows.reduce((max, row) => Math.max(max, row.up, row.down), 0);
  const ceiling = Math.max(48 * 1024, peak * 1.2);

  return (
    <div
      style={{ height }}
      className={dimmed ? "opacity-40 transition-opacity duration-500" : "transition-opacity duration-500"}
    >
      <ResponsiveContainer width="100%" height="100%">
        <AreaChart data={rows} margin={{ top: 6, right: 0, bottom: 0, left: 0 }}>
          <defs>
            <linearGradient id="nano-down" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="#64D2FF" stopOpacity={0.35} />
              <stop offset="100%" stopColor="#64D2FF" stopOpacity={0} />
            </linearGradient>
            <linearGradient id="nano-up" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="#30D158" stopOpacity={0.3} />
              <stop offset="100%" stopColor="#30D158" stopOpacity={0} />
            </linearGradient>
          </defs>
          <YAxis hide domain={[0, ceiling]} />
          <Tooltip
            content={<ChartTooltip />}
            cursor={{ stroke: "rgba(255,255,255,0.12)", strokeWidth: 1 }}
          />
          <Area
            type="monotone"
            dataKey="down"
            stroke="#64D2FF"
            strokeWidth={1.6}
            fill="url(#nano-down)"
            isAnimationActive={false}
            dot={false}
          />
          <Area
            type="monotone"
            dataKey="up"
            stroke="#30D158"
            strokeWidth={1.4}
            fill="url(#nano-up)"
            isAnimationActive={false}
            dot={false}
          />
        </AreaChart>
      </ResponsiveContainer>
    </div>
  );
}
