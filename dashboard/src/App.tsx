import { MemoryGauge } from "./components/MemoryGauge";
import { OpsChart } from "./components/OpsChart";
import { StatTile } from "./components/StatTile";
import { TopKeysChart } from "./components/TopKeysChart";
import { useMetricsSocket } from "./useMetricsSocket";

export default function App() {
  const { latest, history, status } = useMetricsSocket();

  return (
    <div className="app">
      <header className="app-header">
        <h1>kvdb</h1>
        <span className="conn-badge">
          <span className={`conn-dot ${status}`} />
          {status === "open" ? "live" : status === "connecting" ? "connecting…" : "disconnected"}
        </span>
      </header>

      <div className="stat-row">
        <StatTile label="Total ops" value={(latest?.total_ops ?? 0).toLocaleString()} />
        <StatTile label="Ops / sec" value={(latest?.ops_per_sec ?? 0).toFixed(1)} />
        <StatTile label="Keys stored" value={(latest?.key_count ?? 0).toLocaleString()} />
      </div>

      <div className="panel-grid">
        <section className="panel">
          <h2>Operations per second</h2>
          <OpsChart data={history} />
        </section>
        <section className="panel">
          <h2>Memory usage</h2>
          <MemoryGauge bytes={latest?.memory_bytes ?? 0} liveBytes={latest?.memory_live_bytes ?? 0} />
        </section>
      </div>

      <div className="bottom-row">
        <section className="panel">
          <h2>Top 10 most accessed keys</h2>
          <TopKeysChart data={latest?.top_keys ?? []} />
        </section>
      </div>
    </div>
  );
}
