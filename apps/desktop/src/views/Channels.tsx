import { useEffect, useRef, useState } from "react";
import { api, ChannelDto, NodeInfoDto, StoredMsg, UcrEvent, eventKind, eventBody, onUcrEvent } from "../api";
import { VOICE_CODEC_ADPCM, VoiceRx, VoiceTx } from "../audio";
import type { StatsDto } from "../api";

export default function Channels({
  me,
  events,
  infos,
  link,
}: {
  me: number;
  events: (UcrEvent & { at: number })[];
  infos: NodeInfoDto[];
  link: StatsDto | null;
}) {
  const [channels, setChannels] = useState<ChannelDto[]>([]);
  const [active, setActive] = useState("ucr.public");
  const [name, setName] = useState("");
  const [text, setText] = useState("");
  const [history, setHistory] = useState<StoredMsg[]>([]);
  // push-to-talk state
  const [ptt, setPtt] = useState(false);
  const [talker, setTalker] = useState<string | null>(null);
  const [micErr, setMicErr] = useState("");
  // hardware: does this machine even have a mic?
  const [hasMic, setHasMic] = useState<boolean | null>(null);
  const txRef = useRef<VoiceTx | null>(null);
  const rxRef = useRef(new VoiceRx());
  const txIdRef = useRef(0);
  const seqRef = useRef(0);
  const talkerTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const activeRef = useRef(active);
  activeRef.current = active;

  // Detect microphone hardware — no point offering PTT with nothing to
  // capture on. Re-checks when devices change (headset plug/unplug).
  useEffect(() => {
    const check = async () => {
      try {
        const devs = await navigator.mediaDevices?.enumerateDevices();
        setHasMic(devs ? devs.some((d) => d.kind === "audioinput") : null);
      } catch {
        setHasMic(null);
      }
    };
    void check();
    navigator.mediaDevices?.addEventListener?.("devicechange", check);
    return () =>
      navigator.mediaDevices?.removeEventListener?.("devicechange", check);
  }, []);

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
    setMicErr("");
    try {
      await api.joinChannel(me, active);
      await api.sendChannelText(me, active, text.trim());
      setText("");
      setTimeout(() => void refresh(), 400);
    } catch (e) {
      // e.g. "packet too large" — text must fit the link mtu on RF links
      setMicErr(`tx: ${String(e)}`);
    }
  };

  // ---- voice: subscribe to VoiceFrame events for the active channel ----
  useEffect(() => {
    const un = onUcrEvent((e) => {
      if (eventKind(e.event) !== "VoiceFrame" || e.node !== me) return;
      const b = eventBody(e.event);
      if (String(b.channel_name) !== activeRef.current) return;
      rxRef.current.push(
        String(b.from),
        Number(b.codec),
        Number(b.tx_id),
        Number(b.seq),
        Boolean(b.last),
        Uint8Array.from(b.data as number[]),
      );
      setTalker(String(b.callsign));
      if (talkerTimer.current) clearTimeout(talkerTimer.current);
      talkerTimer.current = setTimeout(() => setTalker(null), 500);
    });
    return () => {
      void un.then((f) => f());
    };
  }, [me]);

  const pttDown = async () => {
    setMicErr("");
    try {
      const tx = new VoiceTx();
      txRef.current = tx;
      txIdRef.current = (txIdRef.current + 1) & 0xffff;
      seqRef.current = 0;
      const txId = txIdRef.current;
      await tx.start((data, last) => {
        void api.sendVoice(
          me,
          activeRef.current,
          VOICE_CODEC_ADPCM,
          txId,
          seqRef.current++,
          last,
          Array.from(data),
        );
      });
      setPtt(true);
    } catch (e) {
      txRef.current = null;
      setMicErr(`mic: ${String(e)}`);
    }
  };

  const pttUp = () => {
    const tx = txRef.current;
    if (!tx) return;
    txRef.current = null;
    const txId = txIdRef.current;
    tx.stop((data, last) => {
      void api.sendVoice(
        me,
        activeRef.current,
        VOICE_CODEC_ADPCM,
        txId,
        seqRef.current++,
        last,
        Array.from(data),
      );
    });
    setPtt(false);
  };

  return (
    <div className="flex h-full flex-col gap-4 sm:flex-row">
      <aside className="shrink-0 sm:w-52">
        <div className="mb-2 text-dim">channels</div>
        <div className="flex flex-row flex-wrap gap-1 sm:flex-col sm:flex-nowrap">
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
        {/* voice first — a channel is a frequency, hold to transmit */}
        <div className="flex flex-wrap items-center gap-3 border-b border-line px-3 py-2">
          {/* ADPCM needs ~32kbit/s and ~460B frames — honest gate: a LoRa
              link (1.75kbit/s, 222B) can't carry it. Codec2 voice is the
              firmware path (see ROADMAP). */}
          {link && (link.link_bitrate < 16_000 || link.link_mtu < 464) ? (
            <div className="text-dim">
              [ voice gated ] — {link.link_name} {link.link_bitrate}bps / mtu{" "}
              {link.link_mtu} can't carry adpcm; codec2 voice lands with the
              sx1262 firmware
            </div>
          ) : hasMic === false ? (
            <div className="text-dim">[ no mic found ] — plug in a microphone to transmit</div>
          ) : (
            <button
              onPointerDown={() => void pttDown()}
              onPointerUp={pttUp}
              onPointerLeave={() => ptt && pttUp()}
              onPointerCancel={pttUp}
              onContextMenu={(e) => e.preventDefault()}
              className={`touch-none select-none px-4 py-2 font-bold uppercase tracking-wider ${
                ptt ? "inv" : "border border-line hover:bg-fg hover:text-ink"
              }`}
            >
              {ptt ? "● tx — release to end" : "[ hold to talk ]"}
            </button>
          )}
          <div className="text-dim">
            {talker ? (
              <span className="text-fg">◉ {talker} on the air</span>
            ) : (
              <span>
                #{active} — listening
              </span>
            )}
            {micErr && <span className="ml-2 text-fg">{micErr}</span>}
          </div>
        </div>
        <div className="border-b border-line px-3 py-1 text-dim">
          #{active} <span className="text-dim">— voice + text · flooded · signed</span>
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
          <button className="rounded border border-line bg-panel px-4 text-dim hover:bg-fg hover:text-ink">send</button>
        </form>
      </section>

      <aside className="hidden w-52 shrink-0 text-dim xl:block">
        <div className="mb-2">about #{active}</div>
        <div className="text-xs leading-5">
          hold to talk transmits signed adpcm voice frames, flooded to every
          node on the channel — real-time, never replayed. text floods the
          same way. hop count and verification show on each message.
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
