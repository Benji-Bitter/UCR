import { useEffect, useState } from "react";
import { api, ChannelDto, NodeInfoDto, StoredMsg, UcrEvent, eventKind, eventBody } from "../api";

export default function Channels({
  me,
  events,
  infos,
}: {
  me: number;
  events: (UcrEvent & { at: number })[];
  infos: NodeInfoDto[];
}) {
  const [channels, setChannels] = useState<ChannelDto[]>([]);
  const [active, setActive] = useState("ucr.public");
  const [name, setName] = useState("");
  const [text, setText] = useState("");
  const [history, setHistory] = useState<StoredMsg[]>([]);

  const refresh = async () => {
    setChannels(await api.nodeChannels(me));
    setHistory(await api.nodeChannelHistory(me, active));
  };
  useEffect(() => {
    void refresh();
    const id = setInterval(() => void refresh(), 3000);
    return () => clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [me, active]);

  const chanEvents = events.filter(
    (e) => eventKind(e.event) === "ChannelMessage"
  );

  const send = async () => {
    if (!text.trim()) return;
    await api.joinChannel(me, active);
    await api.sendChannelText(me, active, text.trim());
    setText("");
    setTimeout(() => void refresh(), 400);
  };

  return (
    <div className="flex h-full gap-4">
      <aside className="w-56 shrink-0">
        <div className="mb-2 text-dim">channels</div>
        <div className="flex flex-col gap-1">
          {[...new Set(["ucr.public", "ucr.general", ...channels.map((c) => c.name), active])].map(
            (c) => (
              <button
                key={c}
                onClick={() => setActive(c)}
                className={`rounded px-3 py-1.5 text-left ${
                  c === active ? "bg-panel-2 text-fg" : "bg-panel text-dim hover:text-fg"
                }`}
              >
                #{c}
              </button>
            )
          )}
        </div>
        <form
          className="mt-3 flex gap-1"
          onSubmit={(e) => {
            e.preventDefault();
            if (!name.trim()) return;
            void api.joinChannel(me, name.trim()).then(() => {
              setActive(name.trim());
              setName("");
            });
          }}
        >
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="join channel…"
            className="min-w-0 flex-1 rounded border border-line bg-panel px-2 py-1"
          />
        </form>
      </aside>

      <section className="flex min-w-0 flex-1 flex-col rounded border border-line bg-panel">
        <div className="border-b border-line px-3 py-2">
          #{active} <span className="text-dim">— flooded · signed</span>
        </div>
        <div className="min-h-0 flex-1 overflow-auto p-3">
          {chanEvents.length === 0 && history.length === 0 && (
            <div className="text-dim">no messages yet — send one, watch it flood the mesh</div>
          )}
          {chanEvents.map((e, i) => {
            const b = eventBody(e.event);
            return (
              <div key={i} className="mb-2">
                <span className="text-accent">{String(b.callsign ?? "?")}</span>
                <span className="text-dim"> [{String(b.hops ?? 0)}h{String(b.verified) === "true" ? " ✓" : " ⚠unverified"}]</span>
                <div className="ml-4">{String(b.text ?? "")}</div>
              </div>
            );
          })}
        </div>
        <form
          className="flex gap-2 border-t border-line p-2"
          onSubmit={(e) => {
            e.preventDefault();
            void send();
          }}
        >
          <input
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder={`message #${active}…`}
            className="min-w-0 flex-1 rounded border border-line bg-panel-2 px-3 py-1.5"
          />
          <button className="rounded border border-accent/50 bg-accent/10 px-4 text-accent">send</button>
        </form>
      </section>

      <aside className="hidden w-52 shrink-0 text-dim xl:block">
        <div className="mb-2">about #{active}</div>
        <div className="text-xs leading-5">
          channel messages are flooded to every node within scope and signed by
          the originator. hop count and verification show on each message.
          {infos[me] && (
            <div className="mt-3">
              operating as <span className="text-fg">{infos[me].callsign}</span>
              <br />
              {infos[me].node_id}
            </div>
          )}
        </div>
      </aside>
    </div>
  );
}
