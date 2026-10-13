import { useCallback, useEffect, useRef, useState } from "react";
import { api, IS_TAURI, NodeInfoDto, NodeVisDto, StatsDto, UcrEvent, eventKind, onUcrEvent } from "./api";
import { CITIES } from "./views/Network";
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
  const [mode, setMode] = useState<"idle" | "sim" | "real">("idle");
  const running = mode !== "idle";
  const [topo, setTopo] = useState<NodeVisDto[]>([]);
  const [stats, setStats] = useState<StatsDto | null>(null);
  const [me, setMe] = useState(0);
  const [infos, setInfos] = useState<NodeInfoDto[]>([]);
  const [events, setEvents] = useState<(UcrEvent & { at: number })[]>([]);
  const [nodeCount, setNodeCount] = useState(4);
  const [seed, setSeed] = useState(7);
  const [spacing, setSpacing] = useState(300);
  // real-mode config
  const [form, setForm] = useState<"real" | "sim">(IS_TAURI ? "real" : "sim");
  const [callsign, setCallsign] = useState("");
  const [udpPort, setUdpPort] = useState(4401);
  const [tcpPort, setTcpPort] = useState(4402);
  const [connect, setConnect] = useState("");
  const [serialPath, setSerialPath] = useState("");
  const [posIdx, setPosIdx] = useState("");
  const [startErr, setStartErr] = useState("");
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
        // Refresh node infos — real-mode topo includes remote peers, which
        // aren't local nodes; fall back to their announced callsign/id.
        const inf = await Promise.all(
          t.map((_, i) => api.nodeInfo(i).catch(() => null)),
        );
        if (live)
          setInfos(
            inf.map(
              (x, i) =>
                x ?? {
                  node_id: t[i].node_id,
                  callsign: t[i].callsign,
                  pubkey: "",
                },
            ),
          );
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
    setStartErr("");
    try {
      await api.simStart(nodeCount, spacing, seed);
      await api.subscribeEvents();
      setEvents([]);
      setMe(0);
      setMode("sim");
    } catch (e) {
      setStartErr(String(e));
    }
  }, [nodeCount, spacing, seed]);

  const startReal = useCallback(async () => {
    setStartErr("");
    try {
      const ci = posIdx === "" ? -1 : Number(posIdx);
      await api.realStart({
        callsign,
        desc: "",
        udpPort,
        tcpPort,
        connect: connect
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean),
        serialPath: serialPath || undefined,
        serialBaud: 115200,
        lat: ci >= 0 ? CITIES[ci][2] : undefined,
        lon: ci >= 0 ? CITIES[ci][3] : undefined,
      });
      await api.subscribeEvents();
      setEvents([]);
      setMe(0);
      setMode("real");
    } catch (e) {
      setStartErr(String(e));
    }
  }, [callsign, udpPort, tcpPort, connect, serialPath, posIdx]);

  const stop = useCallback(async () => {
    if (mode === "real") await api.realStop();
    else await api.simStop();
    setMode("idle");
    setTopo([]);
    setStats(null);
    setInfos([]);
  }, [mode]);

  const myEvents = events.filter((e) => e.node === me);

  return (
    <div className="flex h-full flex-col bg-ink text-fg">
      {/* top bar */}
      <header className="flex flex-wrap items-center gap-3 border-b border-line px-4 py-2">
        <div className="flex items-center gap-2">
          <span className="text-base font-bold tracking-widest">UCR://</span>
          <span className="text-dim">universal citizens radio</span>
          {!IS_TAURI && <span className="border border-line px-1 text-dim">web-sim</span>}
        </div>
        <div className="mx-2 h-5 w-px bg-line" />
        {running ? (
          <>
            <span
              className={`border border-line px-1 font-bold ${
                mode === "real" ? "bg-fg text-ink" : "text-dim"
              }`}
            >
              {mode === "real" ? "live" : "sim"}
            </span>
            {mode === "sim" && (
              <>
                <span className="text-dim">node</span>
                <select
                  className="border border-line bg-panel px-2 py-1"
                  value={me}
                  onChange={(e) => setMe(Number(e.target.value))}
                >
                  {topo.map((n, i) => (
                    <option key={n.id} value={i}>
                      {infos[i]?.callsign ?? n.callsign} · {n.node_id.slice(0, 8)}
                    </option>
                  ))}
                </select>
              </>
            )}
            {mode === "real" && (
              <span className="text-dim">
                {topo[0]?.callsign ?? "…"} · {topo[0]?.node_id.slice(0, 8) ?? ""}
              </span>
            )}
            {stats && (
              <span className="ml-auto text-dim">
                tx {stats.medium_tx} · rx {stats.medium_delivered}
                {mode === "sim" && (
                  <>
                    {" "}· drop{" "}
                    {stats.medium_dropped_loss + stats.medium_dropped_collision + stats.medium_dropped_halfduplex} ·
                    air {(stats.airtime_ms / 1000).toFixed(0)}s
                  </>
                )}
              </span>
            )}
            <button
              onClick={stop}
              className="border border-line bg-panel px-3 py-1 uppercase tracking-wider hover:bg-fg hover:text-ink"
            >
              [ stop ]
            </button>
          </>
        ) : form === "real" && IS_TAURI ? (
          <form
            className="flex flex-wrap items-center gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              void startReal();
            }}
          >
            <span className="text-dim">callsign</span>
            <input
              value={callsign}
              onChange={(e) => setCallsign(e.target.value)}
              placeholder="VK2X"
              className="w-20 border border-line bg-panel px-2 py-1 placeholder:text-dim"
            />
            <span className="text-dim">udp</span>
            <input
              type="number"
              value={udpPort}
              onChange={(e) => setUdpPort(Number(e.target.value))}
              className="w-16 border border-line bg-panel px-2 py-1"
            />
            <span className="text-dim">tcp</span>
            <input
              type="number"
              value={tcpPort}
              onChange={(e) => setTcpPort(Number(e.target.value))}
              className="w-16 border border-line bg-panel px-2 py-1"
            />
            <span className="text-dim">peer</span>
            <input
              value={connect}
              onChange={(e) => setConnect(e.target.value)}
              placeholder="host:4402"
              className="w-28 border border-line bg-panel px-2 py-1 placeholder:text-dim"
            />
            <span className="text-dim">serial</span>
            <input
              value={serialPath}
              onChange={(e) => setSerialPath(e.target.value)}
              placeholder="/dev/tty…"
              className="w-24 border border-line bg-panel px-2 py-1 placeholder:text-dim"
            />
            <select
              value={posIdx}
              onChange={(e) => setPosIdx(e.target.value)}
              className="border border-line bg-panel px-2 py-1"
            >
              <option value="">no position</option>
              {CITIES.map((c, i) => (
                <option key={c[0]} value={i}>
                  {c[0]}, {c[1]}
                </option>
              ))}
            </select>
            <button className="inv px-3 py-1 font-bold uppercase tracking-wider hover:opacity-80">
              [ go online ]
            </button>
            <button
              type="button"
              onClick={() => setForm("sim")}
              className="text-dim underline-offset-2 hover:text-fg"
            >
              ·sim
            </button>
            {startErr && <span className="text-fg">err: {startErr}</span>}
          </form>
        ) : (
          <form
            className="flex flex-wrap items-center gap-2"
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
              [ run sim ]
            </button>
            {IS_TAURI && (
              <button
                type="button"
                onClick={() => setForm("real")}
                className="text-dim underline-offset-2 hover:text-fg"
              >
                ·real
              </button>
            )}
            {startErr && <span className="text-fg">err: {startErr}</span>}
          </form>
        )}
      </header>

      <div className="flex min-h-0 flex-1">
        {/* nav */}
        <nav className="flex w-12 flex-col border-r border-line py-2 sm:w-44">
          {NAV.map(([v, label]) => (
            <button
              key={v}
              onClick={() => setView(v)}
              className={`px-2 py-1.5 text-left uppercase tracking-wider sm:px-4 ${
                view === v ? "bg-panel-2 text-fg" : "text-dim hover:bg-panel hover:text-fg"
              }`}
              title={label}
            >
              {view === v ? ">" : " "}
              <span className="hidden sm:inline"> {label}</span>
              <span className="sm:hidden"> {label.slice(0, 3)}</span>
            </button>
          ))}
        </nav>

        {/* view */}
        <main className="min-w-0 flex-1 overflow-x-hidden overflow-y-auto p-2 sm:p-4">
          {!running && view !== "log" ? (
            <EmptyState />
          ) : (
            <>
              {view === "network" && (
                <Network
                  topo={topo}
                  me={me}
                  real={mode === "real"}
                  onSetEnabled={(i, en) => void api.simSetEnabled(i, en)}
                />
              )}
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
      <div>// [ go online ] puts a real node on lan / tcp / serial — actual ucr packets on the wire</div>
      <div className="mt-2 text-xs">[ run sim ] spins up the deterministic rf test world instead</div>
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
