import { useEffect, useRef, useState } from "react";
import { NodeVisDto } from "../api";

/** Live map of the simulated RF world: node positions, heard links, RSSI. */
export default function Network({
  topo,
  me,
  onSetEnabled,
}: {
  topo: NodeVisDto[];
  me: number;
  onSetEnabled: (idx: number, enabled: boolean) => void;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const [selected, setSelected] = useState<number | null>(null);

  useEffect(() => {
    const c = ref.current;
    if (!c) return;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    const { width, height } = c.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    c.width = width * dpr;
    c.height = height * dpr;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, width, height);

    if (!topo.length) return;
    const xs = topo.map((n) => n.x);
    const pad = 60;
    const maxX = Math.max(...xs, 1);
    const sx = (x: number) => pad + (x / (maxX + 1)) * (width - 2 * pad);
    const sy = (y: number) => height / 2 + y;

    // Range ring per node (spacing*1.6 ≈ visible neighbourhood).
    const span = topo.length > 1 ? topo[1].x - topo[0].x : 300;
    const rPix = ((span * 1.6) / (maxX + 1)) * (width - 2 * pad);

    // Links — heard peers.
    for (const n of topo) {
      for (const [peer, rssi] of n.heard) {
        const p = topo[peer];
        if (!p) continue;
        const q = Math.max(0.15, Math.min(1, (rssi + 100) / 50));
        ctx.strokeStyle = `rgba(125,216,125,${(q * 0.8).toFixed(2)})`;
        ctx.lineWidth = p.id === n.id ? 1 : 1;
        ctx.beginPath();
        ctx.moveTo(sx(n.x), sy(n.y));
        ctx.lineTo(sx(p.x), sy(p.y));
        ctx.stroke();
      }
    }

    // Range rings.
    for (const n of topo) {
      ctx.strokeStyle = "rgba(42,46,42,0.8)";
      ctx.setLineDash([3, 5]);
      ctx.beginPath();
      ctx.arc(sx(n.x), sy(n.y), rPix, 0, Math.PI * 2);
      ctx.stroke();
      ctx.setLineDash([]);
    }

    // Nodes.
    topo.forEach((n, i) => {
      const x = sx(n.x);
      const y = sy(n.y);
      ctx.fillStyle = n.enabled ? (i === me ? "#7dd87d" : "#e6e9e4") : "#3a3e3a";
      ctx.beginPath();
      ctx.arc(x, y, i === me ? 9 : 7, 0, Math.PI * 2);
      ctx.fill();
      if (i === selected) {
        ctx.strokeStyle = "#e8b447";
        ctx.lineWidth = 1.5;
        ctx.beginPath();
        ctx.arc(x, y, 13, 0, Math.PI * 2);
        ctx.stroke();
      }
      ctx.fillStyle = "#8a918a";
      ctx.font = "10px ui-monospace";
      ctx.fillText(`${n.callsign}`, x + 12, y - 4);
      ctx.fillText(n.enabled ? `${n.node_id.slice(0, 8)}` : "disabled", x + 12, y + 8);
    });
  }, [topo, me, selected]);

  const sel = selected !== null ? topo[selected] : null;

  return (
    <div className="flex h-full flex-col gap-3">
      <div className="relative min-h-0 flex-1 rounded border border-line bg-panel">
        <canvas
          ref={ref}
          className="absolute inset-0 h-full w-full"
          onClick={(e) => {
            const r = e.currentTarget.getBoundingClientRect();
            const xs = topo.map((n) => n.x);
            const pad = 60;
            const maxX = Math.max(...xs, 1);
            const sx = (x: number) => pad + (x / (maxX + 1)) * (r.width - 2 * pad);
            const sy = (y: number) => r.height / 2 + y;
            const cx = e.clientX - r.left;
            const cy = e.clientY - r.top;
            let best: number | null = null;
            let bd = 30;
            topo.forEach((n, i) => {
              const d = Math.hypot(sx(n.x) - cx, sy(n.y) - cy);
              if (d < bd) {
                bd = d;
                best = i;
              }
            });
            setSelected(best);
          }}
        />
      </div>
      <div className="flex items-center gap-4 rounded border border-line bg-panel px-4 py-2 text-dim">
        {sel ? (
          <>
            <span className="text-fg">{sel.callsign}</span>
            <span>{sel.node_id}</span>
            <span>{sel.enabled ? "radio on" : "radio off"}</span>
            <span>heard: {sel.heard.length} peers</span>
            <button
              onClick={() => onSetEnabled(sel.id, !sel.enabled)}
              className="ml-auto rounded border border-line bg-panel-2 px-3 py-1 text-fg hover:border-warn"
            >
              {sel.enabled ? "disable (partition test)" : "enable"}
            </button>
          </>
        ) : (
          <span>click a node — green links are currently-heard peers (opacity ≈ RSSI)</span>
        )}
        <span className="ml-auto">{topo.length} nodes</span>
      </div>
    </div>
  );
}
