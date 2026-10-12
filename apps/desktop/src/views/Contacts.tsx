import { useEffect, useState } from "react";
import { api, ContactDto, NeighborDto, NodeInfoDto, UcrEvent } from "../api";

export default function Contacts({
  me,
  events,
  infos,
}: {
  me: number;
  events: (UcrEvent & { at: number })[];
  infos: NodeInfoDto[];
}) {
  const [contacts, setContacts] = useState<ContactDto[]>([]);
  const [neighbors, setNeighbors] = useState<NeighborDto[]>([]);
  const [routes, setRoutes] = useState<{ dest: string; next_hop: string; metric: number; hops: number }[]>([]);

  const refresh = async () => {
    setContacts(await api.nodeContacts(me));
    setNeighbors(await api.nodeNeighbors(me));
    setRoutes(await api.nodeRoutes(me));
  };
  useEffect(() => {
    void refresh();
    const id = setInterval(() => void refresh(), 2500);
    return () => clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [me, events.length]);

  const label = (hex: string) => infos.find((i) => i.node_id === hex)?.callsign ?? hex.slice(0, 8);

  return (
    <div className="grid h-full grid-cols-3 gap-4 overflow-auto">
      <section className="rounded border border-line bg-panel p-3">
        <div className="mb-2 text-dim">neighbours ({neighbors.length})</div>
        {neighbors.map((n) => (
          <div key={n.node_id} className="mb-2 rounded bg-panel-2 p-2">
            <div className="flex justify-between">
              <span>{n.callsign || label(n.node_id)}</span>
              <span className={n.bidirectional ? "text-accent" : "text-warn"}>
                {n.bidirectional ? "⇄" : "→"}
              </span>
            </div>
            <div className="text-xs text-dim">
              rssi {n.rssi_ewma.toFixed(0)} dBm · snr {n.snr_ewma.toFixed(0)} · q {n.quality}
            </div>
          </div>
        ))}
        {neighbors.length === 0 && <div className="text-dim">no neighbours heard yet — beacons take a few seconds</div>}
      </section>

      <section className="rounded border border-line bg-panel p-3">
        <div className="mb-2 text-dim">routes ({routes.length})</div>
        {routes.map((r) => (
          <div key={r.dest} className="mb-1 flex justify-between text-xs">
            <span>{label(r.dest)}</span>
            <span className="text-dim">
              via {label(r.next_hop)} · {r.hops}h · m{r.metric}
            </span>
          </div>
        ))}
        {routes.length === 0 && <div className="text-dim">no learned routes — unicast traffic triggers discovery</div>}
      </section>

      <section className="rounded border border-line bg-panel p-3">
        <div className="mb-2 text-dim">contacts ({contacts.length})</div>
        {contacts.map((c) => (
          <div key={c.node_id} className="mb-2 rounded bg-panel-2 p-2">
            <div className="flex justify-between">
              <span>
                {c.callsign} {c.verified && <span className="text-accent">✓</span>}
                {c.trusted && <span className="text-warn"> ★</span>}
              </span>
              <button
                onClick={() => void api.blockContact(me, c.node_id).then(refresh)}
                className={`text-xs ${c.blocked ? "text-danger" : "text-dim hover:text-danger"}`}
              >
                {c.blocked ? "blocked" : "block"}
              </button>
            </div>
            <div className="text-xs text-dim">{c.node_id.slice(0, 16)}</div>
          </div>
        ))}
        {contacts.length === 0 && <div className="text-dim">contacts appear as traffic arrives</div>}
      </section>
    </div>
  );
}
