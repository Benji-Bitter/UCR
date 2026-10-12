import { useState } from "react";
import { api, NodeInfoDto, UcrEvent, eventBody, eventKind } from "../api";

const CATS = ["medical", "fire", "flood", "accident", "vessel", "missing", "other"];

export default function Emergency({
  me,
  events,
  infos,
}: {
  me: number;
  events: (UcrEvent & { at: number })[];
  infos: NodeInfoDto[];
}) {
  const [cat, setCat] = useState("medical");
  const [text, setText] = useState("");
  const [sent, setSent] = useState<number | null>(null);

  const emergencies = events
    .filter((e) => eventKind(e.event) === "Emergency")
    .reverse();

  return (
    <div className="flex h-full gap-4">
      <section className="w-80 shrink-0 rounded border border-danger/40 bg-panel p-4">
        <div className="mb-1 text-lg text-danger">Emergency broadcast</div>
        <div className="mb-3 text-xs text-dim">
          signed · priority-queued · flooded region-wide · relayed even past
          blocks. rate-limited to one per minute.
        </div>
        <form
          className="flex flex-col gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (!text.trim()) return;
            void api.sendEmergency(me, cat, text.trim()).then(setSent);
            setText("");
          }}
        >
          <select
            value={cat}
            onChange={(e) => setCat(e.target.value)}
            className="rounded border border-line bg-panel-2 px-2 py-1.5"
          >
            {CATS.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
          <textarea
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder="situation report…"
            rows={3}
            className="rounded border border-line bg-panel-2 px-2 py-1.5"
          />
          <button className="rounded border border-danger bg-danger/20 py-1.5 text-danger">
            transmit emergency
          </button>
        </form>
        {sent !== null && <div className="mt-2 text-xs text-accent">broadcast queued (id {sent})</div>}
      </section>

      <section className="min-w-0 flex-1 rounded border border-line bg-panel p-3">
        <div className="mb-2 text-dim">received emergencies (all nodes)</div>
        {emergencies.length === 0 && <div className="text-dim">none received</div>}
        {emergencies.map((e, i) => {
          const b = eventBody(e.event);
          return (
            <div key={i} className="mb-2 rounded border border-danger/40 bg-danger/10 p-3">
              <div className="flex justify-between">
                <span className="text-danger">
                  {String(b.category)} · {String(b.callsign)}
                </span>
                <span className="text-xs text-dim">
                  node {e.node} · {String(b.hops)} hops ·{" "}
                  {String(b.verified) === "true" ? <span className="text-accent">signed ✓</span> : <span className="text-warn">unverified ⚠</span>}
                </span>
              </div>
              <div className="mt-1">{String(b.text)}</div>
            </div>
          );
        })}
        {infos.length > 0 && <div className="hidden" />}
      </section>
    </div>
  );
}
