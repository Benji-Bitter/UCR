import { NodeInfoDto, UcrEvent, eventBody, eventKind } from "../api";

export default function Log({
  events,
  infos,
}: {
  events: (UcrEvent & { at: number })[];
  infos: NodeInfoDto[];
}) {
  const list = [...events].reverse().slice(0, 300);
  const callsign = (i: number) => infos[i]?.callsign ?? `n${i}`;

  return (
    <div className="h-full overflow-auto rounded border border-line bg-panel p-3 font-mono text-xs">
      {list.length === 0 && <div className="text-dim">no events — start the simulation</div>}
      {list.map((e, i) => {
        const k = eventKind(e.event);
        const b = eventBody(e.event);
        const cls =
          k === "Emergency" || k === "InvalidPacket"
            ? "text-danger"
            : k === "DirectMessage" || k === "GroupMessage" || k === "SessionEstablished"
              ? "text-accent"
              : "text-dim";
        const detail = Object.entries(b)
          .filter(([key]) => !["pubkey", "desc", "class", "caps", "profile", "seen", "tone_id", "battery", "pos"].includes(key))
          .map(([key, v]) => `${key}=${typeof v === "object" ? JSON.stringify(v) : String(v)}`)
          .join(" ")
          .slice(0, 160);
        const mark =
          k === "Emergency" || k === "InvalidPacket"
            ? "[!]"
            : k === "DirectMessage" || k === "GroupMessage" || k === "SessionEstablished"
              ? "[+]"
              : " · ";
        return (
          <div key={i} className="mb-0.5 flex gap-2">
            <span className="text-line">{new Date(e.at).toLocaleTimeString()}</span>
            <span className={cls}>{mark}</span>
            <span className="text-fg">{callsign(e.node)}</span>
            <span className={cls}>{k}</span>
            <span className="truncate text-dim">{detail}</span>
          </div>
        );
      })}
    </div>
  );
}
