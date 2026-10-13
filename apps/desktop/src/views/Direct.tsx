import { useEffect, useState } from "react";
import { api, NodeInfoDto, NodeVisDto, StoredMsg, UcrEvent, eventKind, eventBody } from "../api";

export default function Direct({
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
  const [peer, setPeer] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [history, setHistory] = useState<StoredMsg[]>([]);

  const peers = topo.filter((_, i) => i !== me).map((n, i2) => ({ idx: i2 >= me ? i2 + 1 : i2, n }));

  useEffect(() => {
    if (!peer) return;
    let live = true;
    const load = async () => {
      const h = await api.nodeDmHistory(me, peer);
      if (live) setHistory(h);
    };
    void load();
    const id = setInterval(() => void load(), 3000);
    return () => {
      live = false;
      clearInterval(id);
    };
  }, [me, peer]);

  const dmEvents = events.filter(
    (e) => eventKind(e.event) === "DirectMessage" && e.node === me
  );
  const deliveries = events.filter(
    (e) => eventKind(e.event) === "Delivery" && e.node === me
  );

  const peerLabel = (hex: string) =>
    infos.find((i) => i.node_id === hex)?.callsign ?? hex.slice(0, 8);

  return (
    <div className="flex h-full flex-col gap-4 sm:flex-row">
      <aside className="shrink-0 sm:w-52">
        <div className="mb-2 text-dim">peers</div>
        <div className="flex flex-row flex-wrap gap-1 sm:flex-col sm:flex-nowrap">
          {peers.map(({ idx, n }) => (
            <button
              key={n.node_id}
              onClick={() => setPeer(n.node_id)}
              className={`rounded px-3 py-1.5 text-left ${
                peer === n.node_id ? "bg-panel-2 text-fg" : "bg-panel text-dim hover:text-fg"
              }`}
            >
              <div>{infos[idx]?.callsign ?? n.callsign}</div>
              <div className="text-xs">{n.node_id.slice(0, 12)}</div>
            </button>
          ))}
          {peers.length === 0 && <div className="text-dim">no other nodes</div>}
        </div>
      </aside>

      <section className="flex min-w-0 flex-1 flex-col rounded border border-line bg-panel">
        <div className="border-b border-line px-3 py-2">
          {peer ? (
            <>
              dm → <span className="text-accent">{peerLabel(peer)}</span>
              <span className="text-dim"> — end-to-end encrypted · acked</span>
            </>
          ) : (
            <span className="text-dim">select a peer</span>
          )}
        </div>
        <div className="min-h-0 flex-1 overflow-auto p-3">
          {!peer && <div className="text-dim">pick a node on the left to open an encrypted thread</div>}
          {peer && (
            <>
              {history.map((m) => (
                <div key={m.msg_id} className={`mb-2 ${m.direction === "Outbound" ? "text-right" : ""}`}>
                  <div
                    className={`inline-block max-w-[70%] rounded px-3 py-1.5 text-left ${
                      m.direction === "Outbound" ? "bg-accent/10 text-fg" : "bg-panel-2"
                    }`}
                  >
                    {m.text}
                    <div className="mt-0.5 text-xs text-dim">{m.delivery}</div>
                  </div>
                </div>
              ))}
              {dmEvents.map((e, i) => {
                const b = eventBody(e.event);
                if (String(b.from) !== peer) return null;
                return (
                  <div key={`e${i}`} className="mb-2">
                    <div className="inline-block max-w-[70%] rounded bg-panel-2 px-3 py-1.5">
                      {String(b.text)}
                      <div className="mt-0.5 text-xs text-dim">decrypted · {peerLabel(String(b.from))}</div>
                    </div>
                  </div>
                );
              })}
              {deliveries.slice(-3).map((e, i) => {
                const b = eventBody(e.event);
                return (
                  <div key={`d${i}`} className="text-xs text-dim">
                    delivery: {String(b.state)} msg {String(b.msg_id ?? "—")}
                  </div>
                );
              })}
            </>
          )}
        </div>
        <form
          className="flex gap-2 border-t border-line p-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (!peer || !text.trim()) return;
            void api.sendDm(me, peer, text.trim());
            setText("");
            setTimeout(async () => setHistory(await api.nodeDmHistory(me, peer)), 500);
          }}
        >
          <input
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder={peer ? "encrypted message…" : "select a peer first"}
            disabled={!peer}
            className="min-w-0 flex-1 rounded border border-line bg-panel-2 px-3 py-1.5 disabled:opacity-40"
          />
          <button disabled={!peer} className="rounded border border-accent/50 bg-accent/10 px-4 text-accent disabled:opacity-40">
            send
          </button>
        </form>
      </section>
    </div>
  );
}
