import { useEffect, useState } from "react";
import { api, GroupDto, NodeInfoDto, NodeVisDto, UcrEvent, eventBody, eventKind } from "../api";

export default function Groups({
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
  const [groups, setGroups] = useState<GroupDto[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [text, setText] = useState("");
  const [invitePeer, setInvitePeer] = useState("");

  const refresh = async () => setGroups(await api.nodeGroups(me));
  useEffect(() => {
    void refresh();
    const id = setInterval(() => void refresh(), 3000);
    return () => clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [me]);

  const invites = events.filter((e) => eventKind(e.event) === "GroupInvite");
  const gmsgs = events.filter((e) => eventKind(e.event) === "GroupMessage");

  return (
    <div className="flex h-full gap-4">
      <aside className="w-56 shrink-0">
        <div className="mb-2 text-dim">groups</div>
        <div className="flex flex-col gap-1">
          {groups.map((g) => (
            <button
              key={g.group_id}
              onClick={() => setActive(g.group_id)}
              className={`rounded px-3 py-1.5 text-left ${
                active === g.group_id ? "bg-panel-2 text-fg" : "bg-panel text-dim hover:text-fg"
              }`}
            >
              <div>{g.name}</div>
              <div className="text-xs">
                {g.members} members · epoch {g.epoch}
              </div>
            </button>
          ))}
          {groups.length === 0 && <div className="text-dim">no groups yet</div>}
        </div>
        <form
          className="mt-3 flex gap-1"
          onSubmit={(e) => {
            e.preventDefault();
            if (!name.trim()) return;
            void api.createGroup(me, name.trim(), "").then(refresh);
            setName("");
          }}
        >
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="new group…"
            className="min-w-0 flex-1 rounded border border-line bg-panel px-2 py-1"
          />
        </form>
      </aside>

      <section className="flex min-w-0 flex-1 flex-col rounded border border-line bg-panel">
        <div className="border-b border-line px-3 py-2">
          {active ? (
            <>
              group <span className="text-accent">{groups.find((g) => g.group_id === active)?.name}</span>
              <span className="text-dim"> — symmetric key · epoch-scoped</span>
            </>
          ) : (
            <span className="text-dim">select or create a group</span>
          )}
        </div>
        <div className="min-h-0 flex-1 overflow-auto p-3">
          {invites.map((e, i) => {
            const b = eventBody(e.event);
            return (
              <div key={`inv${i}`} className="mb-2 rounded border border-warn/40 bg-warn/10 px-3 py-2">
                invite to "{String(b.name)}" from {String(b.callsign)}
                <button
                  onClick={() => void api.acceptGroup(me, String(b.group_id)).then(refresh)}
                  className="ml-3 rounded border border-accent/50 px-2 text-accent"
                >
                  accept
                </button>
              </div>
            );
          })}
          {gmsgs.map((e, i) => {
            const b = eventBody(e.event);
            return (
              <div key={`m${i}`} className="mb-2">
                <span className="text-accent">{String(b.callsign)}</span>
                <div className="ml-4">{String(b.text)}</div>
              </div>
            );
          })}
          {!active && !invites.length && <div className="text-dim">group chat is end-to-end encrypted with a shared group key</div>}
        </div>
        {active && (
          <div className="border-t border-line p-2">
            <form
              className="mb-2 flex gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                if (!invitePeer.trim()) return;
                void api.inviteGroup(me, active, invitePeer.trim());
                setInvitePeer("");
              }}
            >
              <select
                value={invitePeer}
                onChange={(e) => setInvitePeer(e.target.value)}
                className="min-w-0 flex-1 rounded border border-line bg-panel-2 px-2 py-1.5"
              >
                <option value="">invite a peer…</option>
                {topo
                  .filter((_, i) => i !== me)
                  .map((n) => (
                    <option key={n.node_id} value={n.node_id}>
                      {n.callsign} · {n.node_id.slice(0, 10)}
                    </option>
                  ))}
              </select>
              <button className="rounded border border-line px-3 text-dim">invite</button>
            </form>
            <form
              className="flex gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                if (!text.trim()) return;
                void api.sendGroupText(me, active, text.trim());
                setText("");
              }}
            >
              <input
                value={text}
                onChange={(e) => setText(e.target.value)}
                placeholder="group message…"
                className="min-w-0 flex-1 rounded border border-line bg-panel-2 px-3 py-1.5"
              />
              <button className="rounded border border-accent/50 bg-accent/10 px-4 text-accent">send</button>
            </form>
          </div>
        )}
      </section>
      {infos.length > 0 && <div className="hidden" />}
    </div>
  );
}
