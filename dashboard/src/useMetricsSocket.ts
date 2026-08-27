import { useEffect, useRef, useState } from "react";
import type { MetricsPayload, OpsPoint } from "./types";

const MAX_HISTORY = 60; // 60 * 500ms = 30s of history

export type ConnectionState = "connecting" | "open" | "closed";

export function useMetricsSocket() {
  const [latest, setLatest] = useState<MetricsPayload | null>(null);
  const [history, setHistory] = useState<OpsPoint[]>([]);
  const [status, setStatus] = useState<ConnectionState>("connecting");
  const attempt = useRef(0);

  useEffect(() => {
    let socket: WebSocket | null = null;
    let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
    let cancelled = false;

    const connect = () => {
      if (cancelled) return;
      setStatus("connecting");
      const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
      socket = new WebSocket(`${proto}//${window.location.host}/ws`);

      socket.onopen = () => {
        attempt.current = 0;
        setStatus("open");
      };

      socket.onmessage = (ev) => {
        try {
          const payload = JSON.parse(ev.data) as MetricsPayload;
          setLatest(payload);
          setHistory((prev) => {
            const next = [...prev, { t: Date.now(), opsPerSec: payload.ops_per_sec }];
            return next.length > MAX_HISTORY ? next.slice(next.length - MAX_HISTORY) : next;
          });
        } catch {
          // ignore malformed frames
        }
      };

      socket.onclose = () => {
        if (cancelled) return;
        setStatus("closed");
        const delay = Math.min(1000 * 2 ** attempt.current, 10_000);
        attempt.current += 1;
        reconnectTimer = setTimeout(connect, delay);
      };

      socket.onerror = () => {
        socket?.close();
      };
    };

    connect();

    return () => {
      cancelled = true;
      if (reconnectTimer) clearTimeout(reconnectTimer);
      socket?.close();
    };
  }, []);

  return { latest, history, status };
}
