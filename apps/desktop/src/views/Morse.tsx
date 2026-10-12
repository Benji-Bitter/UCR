import { useState } from "react";
import { api, NodeInfoDto, NodeVisDto, UcrEvent, eventBody, eventKind } from "../api";

const TABLE: [string, string][] = [
  ["A", "·−"], ["B", "−···"], ["C", "−·−·"], ["D", "−··"], ["E", "·"],
  ["F", "··−·"], ["G", "−−·"], ["H", "····"], ["I", "··"], ["J", "·−−−"],
  ["K", "−·−"], ["L", "·−··"], ["M", "−−"], ["N", "−·"], ["O", "−−−"],
  ["P", "·−−·"], ["Q", "−−·−"], ["R", "·−·"], ["S", "···"], ["T", "−"],
  ["U", "··−"], ["V", "···−"], ["W", "·−−"], ["X", "−··−"], ["Y", "−·−−"],
  ["Z", "−−··"],
];

function toMorse(text: string): string {
  const map = new Map(TABLE);
  return text
    .toUpperCase()
    .split("")
    .map((c) => (c === " " ? "   " : map.get(c) ?? ""))
    .filter((s) => s !== "")
    .join(" ");
}

export default function Morse({
  me,
  events,
  infos,
  topo,
}: {
  me: number;
  events: (UcrEvent & { at: number })[];
  infos: NodeInfoDto[];
  topo: NodeVisDto[];
}) {
  const [peer, setPeer] = useState("");
  const [text, setText] = useState("");
  const [wpm, setWpm] = useState(20);

  const morseEvents = events.filter((e) => eventKind(e.event) === "Morse");

  return (
    <div className="flex h-full gap-4">
      <section className="flex min-w-0 flex-1 flex-col rounded border border-line bg-panel">
        <div className="border-b border-line px-3 py-2">
          digital morse <span className="text-dim">— text keyed as morse payloads, signed & routed like data</span>
        </div>
        <div className="min-h-0 flex-1 overflow-auto p-3">
          {morseEvents.length === 0 && <div className="text-dim">no morse received</div>}
          {morseEvents.map((e, i) => {
            const b = eventBody(e.event);
            const t = String(b.text ?? "");
            return (
              <div key={i} className="mb-3">
                <div className="text-accent">
                  {String(b.callsign)} <span className="text-dim">@{String(b.wpm)}wpm</span>
                </div>
                <div className="ml-4 font-mono text-lg tracking-widest text-warn">{toMorse(t)}</div>
                <div className="ml-4">{t}</div>
              </div>
            );
          })}
        </div>
        <form
          className="flex gap-2 border-t border-line p-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (!peer || !text.trim()) return;
            void api.sendMorse(me, peer, text.trim(), wpm);
            setText("");
          }}
        >
          <select
            value={peer}
            onChange={(e) => setPeer(e.target.value)}
            className="w-44 rounded border border-line bg-panel-2 px-2 py-1.5"
          >
            <option value="">peer…</option>
            {topo
              .filter((_, i) => i !== me)
              .map((n) => (
                <option key={n.node_id} value={n.node_id}>
                  {infos.find((x) => x.node_id === n.node_id)?.callsign ?? n.callsign}
                </option>
              ))}
          </select>
          <input
            value={text}
            onChange={(e) => setText(e.target.value.toUpperCase().replace(/[^A-Z0-9 ]/g, ""))}
            placeholder="MORSE TEXT"
            className="min-w-0 flex-1 rounded border border-line bg-panel-2 px-3 py-1.5 font-mono tracking-widest"
          />
          <select
            value={wpm}
            onChange={(e) => setWpm(Number(e.target.value))}
            className="rounded border border-line bg-panel-2 px-2 py-1.5"
          >
            {[10, 15, 20, 25, 30].map((w) => (
              <option key={w} value={w}>
                {w}wpm
              </option>
            ))}
          </select>
          <button disabled={!peer} className="rounded border border-accent/50 bg-accent/10 px-4 text-accent disabled:opacity-40">
            key
          </button>
        </form>
      </section>

      <aside className="hidden w-56 shrink-0 rounded border border-line bg-panel p-3 text-xs xl:block">
        <div className="mb-2 text-dim">reference</div>
        <div className="grid grid-cols-2 gap-x-3 gap-y-0.5 font-mono">
          {TABLE.map(([c, m]) => (
            <div key={c} className="flex justify-between">
              <span>{c}</span>
              <span className="text-warn">{m}</span>
            </div>
          ))}
        </div>
        {text && (
          <>
            <div className="mt-3 mb-1 text-dim">preview</div>
            <div className="break-all font-mono text-warn">{toMorse(text)}</div>
          </>
        )}
      </aside>
    </div>
  );
}
