import { useCallback, useEffect, useRef, useState } from "react";
import { api, IS_TAURI, NodeInfoDto, NodeVisDto, StatsDto, UcrEvent, eventKind, onUcrEvent } from "./api";
import Channels from "./views/Channels";
import Contacts from "./views/Contacts";
import Direct from "./views/Direct";
import Emergency from "./views/Emergency";
import Groups from "./views/Groups";
import Morse from "./views/Morse";
import Network from "./views/Network";
import Log from "./views/Log";

type View = "network" | "channels" | "direct" | "groups" | "contacts" | "emergency" | "morse" | "log";

const NAV: [View, string][] = [
  ["network", "Network"],
  ["channels", "Channels"],
  ["direct", "Direct"],
  ["groups", "Groups"],
  ["contacts", "Contacts"],
  ["emergency", "Emergency"],
  ["morse", "Morse"],
  ["log", "Log"],
];

export default function App() {
  const [view, setView] = useState<View>("network");
  const [running, setRunning] = useState(false);
  const [topo, setTopo] = useState<NodeVisDto[]>([]);
  const [stats, setStats] = useState<StatsDto | null>(null);
  const [me, setMe] = useState(0);
  const [infos, setInfos] = useState<NodeInfoDto[]>([]);
  const [events, setEvents] = useState<(UcrEvent & { at: number })[]>([]);
  const [nodeCount, setNodeCount] = useState(4);
  const [seed, setSeed] = useState(7);
  const [spacing, setSpacing] = useState(300);
  const eventsRef = useRef(events);
  eventsRef.current = events;

  // Poll topology + stats while running.
  useEffect(() => {
    if (!running) return;
    let live = true;
    const tick = async () => {
      try {
        const [t, s] = await Promise.all([api.simTopology(), api.simStats()]);
        if (!live) return;
        setTopo(t);
        setStats(s);
        // Refresh node infos when count changes.
        const inf = await Promise.all(t.map((_, i) => api.nodeInfo(i)));
        if (live) setInfos(inf);
      } catch {
        /* sim may be stopping */
      }
    };
    void tick();
    const id = setInterval(tick, 1000);
    return () => {
      live = false;
      clearInterval(id);
    };
  }, [running]);

  // Event stream subscription.
  useEffect(() => {
    if (!running) return;
    const un = onUcrEvent((e) => {
      setEvents((prev) => [...prev.slice(-400), { ...e, at: Date.now() }]);
    });
    return () => {
      void un.then((f) => f());
    };
  }, [running]);

  const start = useCallback(async () => {
    await api.simStart(nodeCount, spacing, seed);
    await api.subscribeEvents();
    setEvents([]);
    setMe(0);
    setRunning(true);
  }, [nodeCount, spacing, seed]);

  const stop = useCallback(async () => {
    await api.simStop();
    setRunning(false);
    setTopo([]);
    setStats(null);
    setInfos([]);
  }, []);

  const myEvents = events.filter((e) => e.node === me);

  return (
    <div className="flex h-full flex-col bg-ink text-fg">
      {/* top bar */}
      <header className="flex items-center gap-3 border-b border-line px-4 py-2">
        <div className="flex items-center gap-2">
          <span className="text-base font-bold tracking-widest">UCR://</span>
          <span className="text-dim">universal citizens radio</span>
          {!IS_TAURI && <span className="border border-line px-1 text-dim">web-sim</span>}
        </div>
        <div className="mx-2 h-5 w-px bg-line" />
        {running ? (
          <>
            <span className="text-dim">node</span>
            <select
              className="border border-line bg-panel px-2 py-1"
              value={me}
              onChange={(e) => setMe(Number(e.target.value))}
            >
              {topo.map((n, i) => (
                <option key={n.id} value={i}>
                  {infos[i]?.callsign ?? `n${i}`} · {n.node_id.slice(0, 8)}
                </option>
              ))}
            </select>
            {stats && (
              <span className="ml-auto text-dim">
                tx {stats.medium_tx} · rx {stats.medium_delivered} · drop{" "}
                {stats.medium_dropped_loss + stats.medium_dropped_collision + stats.medium_dropped_halfduplex} · air{" "}
                {(stats.airtime_ms / 1000).toFixed(0)}s
              </span>
            )}
            <button
              onClick={stop}
              className="border border-line bg-panel px-3 py-1 uppercase tracking-wider hover:bg-fg hover:text-ink"
            >
              [ stop ]
            </button>
          </>
        ) : (
          <form
            className="flex items-center gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              void start();
            }}
          >
            <span className="text-dim">nodes</span>
            <input
              type="number"
              min={2}
              max={12}
              value={nodeCount}
              onChange={(e) => setNodeCount(Number(e.target.value))}
              className="w-14 border border-line bg-panel px-2 py-1"
            />
            <span className="text-dim">spacing m</span>
            <input
              type="number"
              min={100}
              max={2000}
              step={50}
              value={spacing}
              onChange={(e) => setSpacing(Number(e.target.value))}
              className="w-20 border border-line bg-panel px-2 py-1"
            />
            <span className="text-dim">seed</span>
            <input
              type="number"
              value={seed}
              onChange={(e) => setSeed(Number(e.target.value))}
              className="w-16 border border-line bg-panel px-2 py-1"
            />
            <button className="inv px-3 py-1 font-bold uppercase tracking-wider hover:opacity-80">
              [ run ]
            </button>
          </form>
        )}
      </header>

      <div className="flex min-h-0 flex-1">
        {/* nav */}
        <nav className="flex w-44 flex-col border-r border-line py-2">
          {NAV.map(([v, label]) => (
            <button
              key={v}
              onClick={() => setView(v)}
              className={`px-4 py-1.5 text-left uppercase tracking-wider ${
                view === v ? "bg-panel-2 text-fg" : "text-dim hover:bg-panel hover:text-fg"
              }`}
            >
              {view === v ? "> " : "  "}
              {label}
            </button>
          ))}
        </nav>

        {/* view */}
        <main className="min-w-0 flex-1 overflow-auto p-4">
          {!running && view !== "log" ? (
            <EmptyState />
          ) : (
            <>
              {view === "network" && <Network topo={topo} me={me} onSetEnabled={(i, en) => void api.simSetEnabled(i, en)} />}
              {view === "channels" && <Channels me={me} events={myEvents} infos={infos} />}
              {view === "direct" && <Direct me={me} events={events} infos={infos} topo={topo} />}
              {view === "groups" && <Groups me={me} events={myEvents} infos={infos} topo={topo} />}
              {view === "contacts" && <Contacts me={me} events={myEvents} infos={infos} />}
              {view === "emergency" && <Emergency me={me} events={events} infos={infos} />}
              {view === "morse" && <Morse me={me} events={myEvents} infos={infos} topo={topo} />}
              {view === "log" && <Log events={events} infos={infos} />}
            </>
          )}
        </main>
      </div>
    </div>
  );
}

export function EmptyState() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 text-dim">
      <pre className="text-fg leading-tight">
{String.raw`██    ██  ██████  ██████
██    ██ ██       ██   ██
██    ██ ██       ██████
██    ██ ██       ██   ██
 ██████   ██████  ██   ██`}
      </pre>
      <div>// configure mesh parameters &gt; run — no radio hardware needed</div>
      <div className="mt-2 text-xs">everything here runs the real protocol stack, end to end</div>
    </div>
  );
}

export function eventLabel(e: UcrEvent): string {
  const k = eventKind(e.event);
  const colors: Record<string, string> = {
    Emergency: "text-danger",
    InvalidPacket: "text-danger",
    DirectMessage: "text-accent",
    GroupMessage: "text-accent",
    ChannelMessage: "text-fg",
    SessionEstablished: "text-warn",
    Delivery: "text-dim",
  };
  return colors[k] ?? "text-dim";
}

export { eventKind };
