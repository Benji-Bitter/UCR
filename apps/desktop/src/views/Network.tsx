import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { NodeVisDto } from "../api";
import { WORLD_CITIES } from "../worldcities";

/* ------------------------------------------------------------------ *
 *  ASCII globe — orthographic projection of a rasterised world map.
 *  Nodes sit at coarse city locations (never precise coordinates),
 *  links draw as great-circle dot trails. Drag to look around.
 * ------------------------------------------------------------------ */

// --- world raster ---------------------------------------------------
// Natural Earth 110m land, rasterised offline to a 360x180 cell grid
// (1 deg cells) and bit-packed into base64. ~8KB of real coastline data.
const MW = 360; // cells wide  (lon -180..180)
const MH = 180; // cells high  (lat   90..-90)

const LAND_B64 =
  "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAH/4AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADf//8AAf///wAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAB////D//////n/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA/gf/x////////wAAAAAbgAAB4AAAAAAPwAAAAAAAAAAAAAAAAAAAAAAAAAAAf//8A///////+AAAAB/ngAAAAAAAAAAH8AAAAAAAAAAAAAAAAAAAAAAAAcH3Hv/5////////8AAAAA/gAAAAAAAAAAAAD4AAAAAAAAAAAAAAAAAAAAAAcIAAQP/AH///////+AAAAAPCAAAAAAAAAAAAAYAAAAAAAAAAAAAAAAAAAAADwDAc+f+AP///////4AAAAAAAAAAAAAB4AAAAB/8AAAAAAAAAAAAAAAAAAAAAH9wc374AAAf/////8AAAAAAAAAAAAH4AAAA////AAAB/gAAAAAAAAAAAAAAAAwAAA/QAAAH/////8AAAAAAAAAAAAOAAAAP///wAAAAAAAAAAAAAAAAAAAAP8AYe+c3AAAD/////4AAAAAAAAAAAA4AAAH////++HgAOAAAAAAAAAAAAAAAfv+4+w/4gAAB/////wAAAAAAAAAAADwAHgH///////4AfgAAAAAgAAAAAAAAPf/4O4//9AAB/////wAAAAAAAAAAADwAPb////////4m//gAAAAAAAf+AAAAAA/+A8f//4AB////8gAAAAAAA/gAAAAAfv/////////////8AAAAAB///+H//H/+PeDwf+AA3////AAAAAAA//4AAAA8fv///////////////P8gAf////////A4CPz4H8AAP///wAAAAAAD///wADH/n3/////////////////8AD////////////34Z/wA///4AAAAAAAP///+M////n/////////////////+4E/////////////gD/8Af//gAAAAAAAf//5+P////f/////////////////fw/////////////6AD8YAf/wAAf8AAAA/8f+D///////////////////////AgEf///////////zw7/AAP/gAAP4AAAB/4/+f//////////////////////8AMAf//////////+DIA/gAH/gAAAAAAAH/z/////////////////////////+AAH///////////8AwgOAAD+AAAAAAAA//H/////////////////////////6AAP///////////wAA/AAAB+AAAAAAAB/+H//////////////////////nP+AAAH//z////////wAA/wAAAOAAAAAAAB//D/////////////////////+A/4AAAA/zAD///////gAA/4wAAAAAAAAAAB//Ab///////////////////+eDgAAAAABwAA///////4AA/94AAAAAAAAAYA5+A///////////////////4AAHAAAAAADcAAD//////4AAf/8AAAAAAAAA8AC+C///////////////////wAAfgAAAAAMAAAA///////wAf/8AAAAAAAAA4AO8H///////////////////AAA/gAAAABgAAAA///////8A///AAAAAAAAA8ANwH//////////////////8AAB/AAAAAEAAAAAf///////z///4AAAAAAAHOAEDv//////////////////+AAA+AAAAAAAAAABH///////x///8AAAAAAAHHAf/////////////////////6AA8AAAAAAAAAAAD///////x///8AAAAAAAGPz//////////////////////6AAwAAAAAAAAAAAD///////9///6AAAAAAAAPj//////////////////////6AAQAAAAAAAAAAAC///////////MAAAAAAAAYf//////////////////////7AAAAAAAAAAAAAABv////////+MNAAAAAAAAD///////////////////////zAAAAAAAAAAAAAAAH////////7gfgAAAAAAAf///////////////////////yAAAAAAAAAAAAAAAL/////////gCgAAAAAAAH///////////////////////iAAAAAAAAAAAAAAAP/////////pAAAAAAAAAD/////uP/x//////////////DAAAAAAAAAAAAAAAP/////////+AAAAAAAAAB//v//Gf/B/////////////+AAAAAAAAAAAAAAAAP////////8wAAAAAAAAAB//H/+AP+P/////////////8CAAAAAAAAAAAAAAAP////////wAAAAAAAAADj/jz/8AD/H/////////////4HgAAAAAAAAAAAAAAP////////gAAAAAAAAAH/4Bw/8AA/B////////////+APAAAAAAAAAAAAAAAP////////gAAAAAAAAAH/wA8f8Ph/g////////////8AAAAAAAAAAAAAAAAAP///////8AAAAAAAAAAH/gMHfJ///x///////////v4AMAAAAAAAAAAAAAAAP///////8AAAAAAAAAAH/AMCOP///h//////////+JwAMAAAAAAAAAAAAAAAH///////4AAAAAAAAAAH/AAAHH///g//////////8BwAIAAAAAAAAAAAAAAAH///////wAAAAAAAAAAH+AAYGH///w//////////+w4A4AAAAAAAAAAAAAAAD///////wAAAAAAAAAAAgf+ACBs//////////////g4D4AAAAAAAAAAAAAAAB///////wAAAAAAAAAAAj/+AAAA//////////////AYf4AAAAAAAAAAAAAAAA///////gAAAAAAAAAAB//8AAAA//////////////Ah2AAAAAAAAAAAAAAAAAP/////+AAAAAAAAAAAD//+AAAB//////////////gDwAAAAAAAAAAAAAAAAAH/////8AAAAAAAAAAAH///4GAB//////////////gDAAAAAAAAAAAAAAAAAAG/////4AAAAAAAAAAAP///8P4h//////////////wCAAAAAAAAAAAAAAAAAACf////4AAAAAAAAAAAP////v////////////////gAAAAAAAAAAAAAAAAAAABP//hgYAAAAAAAAAAAP//////3//P///////////wAAAAAAAAAAAAAAAAAAAAv//AAYAAAAAAAAAAAf//////9//H///////////wAAAAAAAAAAAAAAAAAAAB3/+AAcAAAAAAAAAAB///////4//h///////////gAAAAAAAAAAAAAAAAAAAAR/+AANAAAAAAAAAAD///////8//wH//////////AAAAAAAAAAAAAAAAAAAAAJ/+AAEAAAAAAAAAAH///////+f/0B/////////+AAAAAAAAAAAAAAAAAAAAAI/8AAAAAAAAAAAAAH///////+f/44Af///////+QAAAAAAAAAAAAAAAAAAAAAf8AAAAAAAAAAAAAP////////H//+AP///////4gAAAAAAAAAAAAAAAAAAAAAP8AAuAAAAAAAAAAP////////H///AD///v///ggAAAAAAAAAAAAAAAAAAAAAH+AABgAAAAAAAAAf////////n//+AD//4P//YAAAAAAAAAAAAAAAAAAAAAAAP+A4AYAAAAAAAAAP////////j//+AAf/4H/+AAAAAAAAAAAAAAAAgAAAAAAAH/B4ABwAAAAAAAAP////////h//8AAf/gD/8YAAAAAAAAAAAAAAAAAAAAAAAD/nwAD9AAAAAAAAP////////x//4AAf/AD/8QAAAAAAAAAAAAAAAAAAAAAAAAf/wAAAAAAAAAAAP////////4//gAAf+AD/+AAwAAAAAAAAAAAAAAAAAAAAAAH/wAAAAAAAAAAAf////////4f+AAAf8AD//AAwAAAAAAAAAAAAAAAAAAAAAAAH/AAAAAAAAAAAf////////8f8AAAPwAAP/gAwAAAAAAAAAAAAAAAAAAAAAAAD/gAAAAAAAAAAf////////+fgAAAPwAAP/gAwAAAAAAAAAAAAAAAAAAAAAAAA/AAAAAAAAAAAf/////////eAAAAHwAAP/gAMAAAAAAAAAAAAAAAAAAAAAAAAHgAAAAAAAAAAf/////////gAAAAHwAAM/gACAAAAAAAAAAAAAAAAAAAAAAAADABAAAAAAAAAP/////////gYAAADwAAEfgAJAAAAAAAAAAAAAAAAAAAAAAAADgPfKAAAAAAAH/////////34AAADwAAIOABAAAAAAAAAAAAAAAAAAAAAAAAAAyPf+AAAAAAAD//////////4AAADoAAIEACBAAAAAAAAAAAAAAAAAAAAAAAAA8///AAAAAAAB//////////wAAABIAAMAAAFAAAAAAAAAAAAAAAAAAAAAAAAAE///gAAAAAAB//////////wAAAAMAAEAAALgAAAAAAAAAAAAAAAAAAAAAAAAAf//wAAAAAAAf/////////gAAAAMAADAAMCAAAAAAAAAAAAAAAAAAAAAAAAAA////gAAAAAAP/B///////gAAAAAAADgAOAAAAAAAAAAAAAAAAAAAAAAAAAAAf///wAAAAAACAA///////AAAAAAAAxgA+AAAAAAAAAAAAAAAAAAAAAAAAAAAf///4AAAAAAAAAD/////+AAAAAAAAZgB4AAAAAAAAAAAAAAAAAAAAAAAAAAB////4AAAAAAAAAD/////8AAAAAAAAMwH8AAAAAAAAAAAAAAAAAAAAAAAAAAB////8AAAAAAAAAH/////4AAAAAAAAHQf8AIAAAAAAAAAAAAAAAAAAAAAAAAD////8AAAAAAAAAH/////gAAAAAAAAHgf88IAAAAAAAAAAAAAAAAAAAAAAAAD////+AAAAAAAAAH/////AAAAAAAAADwf8AAgAAAAAAAAAAAAAAAAAAAAAAAH/////4AAAAAAAAH/////AAAAAAAAABwP5wBwAAAAAAAAAAAAAAAAAAAAAAAH/////6AAAAAAAAD////+AAAAAAAAAB8P5wATwAAAAAAAAAAAAAAAAAAAAAAD//////4AAAAAAAB////8AAAAAAAAAA8AxQif+AAAAAAAAAAAAAAAAAAAAAAH//////8AAAAAAAB////4AAAAAAAAAAcAAIAD/gAAAAAAAAAAAAAAAAAAAAAH///////gAAAAAAA////4AAAAAAAAAAMAAIAI/ywAAAAAAAAAAAAAAAAAAAAH///////gAAAAAAA////4AAAAAAAAAADgAAAI/8BAAAAAAAAAAAAAAAAAAAAD///////gAAAAAAAf///4AAAAAAAAAAB+AAAA/4AIAAAAAAAAAAAAAAAAAAAB///////gAAAAAAAf///4AAAAAAAAAAAAmogAOMAAAAAAAAAAAAAAAAAAAAAB///////AAAAAAAAf///8AAAAAAAAAAAABCAAAGADAAAAAAAAAAAAAAAAAAAA///////AAAAAAAAf///8AAAAAAAAAAAAAAAAAAgAAAAAAAAAAAAAAAAAAAAAf/////+AAAAAAAAP///8AAAAAAAAAAAAAAAgCAAAAAAAAAAAAAAAAAAAAAAAf/////8AAAAAAAAf///+AQAAAAAAAAAAAAB+CAAAAAAAAAAAAAAAAAAAAAAAP/////4AAAAAAAAf///+AQAAAAAAAAAAAAD8DAAAAAAAAAAAAAAAAAAAAAAAP/////4AAAAAAAA////+AwAAAAAAAAAAAA38DgAAAAAAAAAAAAAAAAAAAAAAH/////4AAAAAAAA////8BwAAAAAAAAAAAD/8DgAAAAAAAAAAAAAAAAAAAAAAB/////4AAAAAAAA////8PwAAAAAAAAAAAD//HwAABABAAAAAAAAAAAAAAAAAAf////4AAAAAAAA////wPgAAAAAAAAAAAP//3wAAAACAAAAAAAAAAAAAAAAAAP////wAAAAAAAA////APgAAAAAAAAAAAP///wAAAAAAAAAAAAAAAAAAAAAAAP////wAAAAAAAAf//+APgAAAAAAAAAAAf///4AAAAAAAAAAAAAAAAAAAAAAAP////wAAAAAAAAf//+APgAAAAAAAAAAD////+AAIAAAAAAAAAAAAAAAAAAAAP////gAAAAAAAAP//+AfAAAAAAAAAAAf////+AAEAAAAAAAAAAAAAAAAAAAAP////AAAAAAAAAP///AfAAAAAAAAAAA//////AAAAAAAAAAAAAAAAAAAAAAAP///4AAAAAAAAAP//+APAAAAAAAAAAA//////gAAAAAAAAAAAAAAAAAAAAAAf///gAAAAAAAAAH//+AOAAAAAAAAAAB//////wAAAAAAAAAAAAAAAAAAAAAAf///AAAAAAAAAAH//4AEAAAAAAAAAAA//////4AAAAAAAAAAAAAAAAAAAAAAf//+AAAAAAAAAAH//4AAAAAAAAAAAAB//////4AAAAAAAAAAAAAAAAAAAAAAf//+AAAAAAAAAAH//4AAAAAAAAAAAAA//////4AAAAAAAAAAAAAAAAAAAAAAf//+AAAAAAAAAAD//wAAAAAAAAAAAAAf/////8AAAAAAAAAAAAAAAAAAAAAAf//8AAAAAAAAAAB//gAAAAAAAAAAAAAf/////4AAAAAAAAAAAAAAAAAAAAAA///8AAAAAAAAAAB//gAAAAAAAAAAAAAf/////4AAAAAAAAAAAAAAAAAAAAAA///4AAAAAAAAAAA//AAAAAAAAAAAAAAP/////4AAAAAAAAAAAAAAAAAAAAAAf//wAAAAAAAAAAA/+AAAAAAAAAAAAAAP/AP//wAAAAAAAAAAAAAAAAAAAAAA///gAAAAAAAAAAA/4AAAAAAAAAAAAAAf8AG//gAAAAAAAAAAAAAAAAAAAAAA//3AAAAAAAAAAAAYAAAAAAAAAAAAAAAOAAF//gAAAAAAAAAAAAAAAAAAAAAB//4AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA//AAABAAAAAAAAAAAAAAAAAAB//4AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAP/AAAAgAAAAAAAAAAAAAAAAAD//4AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAP/AAAAQAAAAAAAAAAAAAAAAAB//gAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABYAAAAcAAAAAAAAAAAAAAAAAB/8AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA4AAAAAAAAAAAAAAAAAD/8AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAYAAAAAAAAAAAAAAAAAD/gAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAcAAADQAAAAAAAAAAAAAAAAAD/4AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAcAAAHAAAAAAAAAAAAAAAAAAB/gAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAIAAAOAAAAAAAAAAAAAAAAAAD/gAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA4AAAAAAAAAAAAAAAAAAH+AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAB4AAAAAAAAAAAAAAAAAAH+AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAwAAAAAAAAAAAAAAAAAAH/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAP+AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAP8AAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAH4AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAH4BwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD8AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAYAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAABwAAAAAAAAAAAAAAAAAAH4AAAAAAB4HgAD8AAAAAAAAAAAAAAAAAAAAAAAAADgAAAAAAAAAAAAAAAAAA//AAAD//////////wAAAAAAAAAAAAAAAAAAAAAAADgAAAAAAAAAAAAAAAAAf///4Af///////////AAAAAAAAAAAAAAAAAAAAAAAz4AAAAAAAAAAAAAAAPz////8B/////////////wAAAAAAAAAAAAAAAAAAAAA78AAAAAAAAAAA8/8///////wf/////////////+AAAAAAAAAAAAAAAAAAAAH9+AAAAAAAAZ////////////w////////////////4AAAAAAAAAAAAAAAAAAAB+AAAAAAAB//////////////////////////////4AAAAAAAAABmAAB8f+HB/+AAAAAAAP//////////////////////////////gAAAAAAAAP/c/oAf/////4AAAAAAAP/////////////////////////////8AAAAAAAD/////////////gAAAAAAB//////////////////////////////wAAAAAAAD////////////wAAAAAAP///////////////////////////////wAAAAAD/////////////4AAAAAAD////////////////////////////////wAAAADh/////////////AAAAB8A/////////////////////////////////+AAAAA4Af///////////gAAAH+AA///////////////////////////////+AAAAAAAAH///////////+A/A/wAA///////////////////////////////8AAAAAAAf//////////////gAAAf////////////////////////////////+AAAAAAAH///////////////h////////////////////////////////////wAAAAAAP/////////////////////////////////////////////////////gA+/gAAH/////////////////////////////////////////////////////8/////v//////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////";

const GRID: Uint8Array = (() => {
  const bin = atob(LAND_B64);
  const g = new Uint8Array(MW * MH);
  for (let i = 0; i < g.length; i++)
    g[i] = (bin.charCodeAt(i >> 3) >> (7 - (i & 7))) & 1;
  return g;
})();

// --- coarse node locations --------------------------------------------
// Deterministic city assignment per node — city-level only, never precise.
// [city, country, lat, lon] — exported for the real-mode position picker
export const CITIES: [string, string, number, number][] = [
  ["sydney", "australia", -33.9, 151.2],
  ["perth", "australia", -31.9, 115.9],
  ["auckland", "new zealand", -36.8, 174.8],
  ["tokyo", "japan", 35.7, 139.7],
  ["singapore", "singapore", 1.4, 103.8],
  ["jakarta", "indonesia", -6.2, 106.8],
  ["mumbai", "india", 19.1, 72.9],
  ["dubai", "uae", 25.2, 55.3],
  ["moscow", "russia", 55.8, 37.6],
  ["berlin", "germany", 52.5, 13.4],
  ["london", "uk", 51.5, -0.1],
  ["madrid", "spain", 40.4, -3.7],
  ["lagos", "nigeria", 6.5, 3.4],
  ["cairo", "egypt", 30.0, 31.2],
  ["johannesburg", "south africa", -26.2, 28.0],
  ["new york", "usa", 40.7, -74.0],
  ["los angeles", "usa", 34.1, -118.2],
  ["vancouver", "canada", 49.3, -123.1],
  ["mexico city", "mexico", 19.4, -99.1],
  ["sao paulo", "brazil", -23.6, -46.6],
  ["buenos aires", "argentina", -34.6, -58.4],
  ["santiago", "chile", -33.4, -70.7],
  ["honolulu", "usa", 21.3, -157.8],
  ["nairobi", "kenya", -1.3, 36.8],
];

// --- sphere math -------------------------------------------------------
type V3 = [number, number, number];
const D2R = Math.PI / 180;

function ll2v(lat: number, lon: number): V3 {
  const la = lat * D2R;
  const lo = lon * D2R;
  return [Math.cos(la) * Math.cos(lo), Math.sin(la), Math.cos(la) * Math.sin(lo)];
}

// yaw around Y, pitch around X — expanded to a per-frame matrix in draw()

// great-circle interpolation on the unit sphere
function slerp(a: V3, b: V3, t: number): V3 {
  const dot = Math.max(-1, Math.min(1, a[0] * b[0] + a[1] * b[1] + a[2] * b[2]));
  const th = Math.acos(dot);
  if (th < 1e-4) return a;
  const s = Math.sin(th);
  const k0 = Math.sin((1 - t) * th) / s;
  const k1 = Math.sin(t * th) / s;
  return [a[0] * k0 + b[0] * k1, a[1] * k0 + b[1] * k1, a[2] * k0 + b[2] * k1];
}

// precomputed unit vectors for every land cell (static grid)
const LANDV: V3[] = (() => {
  const v: V3[] = [];
  for (let r = 0; r < MH; r++) {
    const lat = 90 - ((r + 0.5) / MH) * 180;
    for (let c = 0; c < MW; c++) {
      if (!GRID[r * MW + c]) continue;
      const lon = ((c + 0.5) / MW) * 360 - 180;
      v.push(ll2v(lat, lon));
    }
  }
  return v;
})();

// sparse labels — dense regions get one group name, nothing stacks
const COUNTRIES: [string, number, number][] = [
  ["united states", 39, -98], ["canada", 57, -108],
  ["europe", 47, 14],
  ["japan + korea", 37, 132],
  ["australia", -25, 134], ["new zealand", -42, 172],
];
const COUNTRYV: [string, V3][] = COUNTRIES.map(([n, la, lo]) => [n, ll2v(la, lo)]);

// every world city (>15k pop), precomputed onto the sphere, pop-sorted
interface WC {
  name: string;
  cc: string;
  lat: number;
  lon: number;
  pop: number;
  cap: boolean;
  v: V3;
}
const WCITIES: WC[] = WORLD_CITIES.trim()
  .split("\n")
  .map((l) => {
    const [name, cc, la, lo, pop, cap] = l.split("|");
    const lat = +la;
    const lon = +lo;
    return { name, cc, lat, lon, pop: +pop, cap: cap === "1", v: ll2v(lat, lon) };
  });

// Nearest known city to an announced coarse position — pins read like
// "near tokyo, jp" rather than raw coordinates.
function placeOf(lat: number, lon: number): [string, string] {
  let best: WC | null = null;
  let bd = Infinity;
  const v = ll2v(lat, lon);
  for (const c of WCITIES) {
    const d =
      (c.v[0] - v[0]) ** 2 + (c.v[1] - v[1]) ** 2 + (c.v[2] - v[2]) ** 2;
    if (d < bd) {
      bd = d;
      best = c;
    }
  }
  if (best && bd < 0.05) return [`near ${best.name}`, best.cc];
  return [`${lat.toFixed(2)}°`, `${lon.toFixed(2)}°`];
}

// -----------------------------------------------------------------------

export default function Network({
  topo,
  me,
  onSetEnabled,
  real = false,
}: {
  topo: NodeVisDto[];
  me: number;
  onSetEnabled: (idx: number, enabled: boolean) => void;
  /** real mode — peers aren't ours to disable, positions are announced */
  real?: boolean;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const wrap = useRef<HTMLDivElement>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const view = useRef({
    yaw: -1.9,
    pitch: 0, // equator loads perfectly horizontal
    zoom: 1,
    drag: false,
    lx: 0,
    ly: 0,
    moved: 0,
    fly: null as { yaw: number; pitch: number; zoom: number } | null,
  });
  const topoRef = useRef(topo);
  topoRef.current = topo;
  const selRef = useRef(selected);
  selRef.current = selected;
  const meRef = useRef(me);
  meRef.current = me;
  const pins = useRef<{ i: number; sx: number; sy: number }[]>([]);

  // Deterministic coarse location per node — city-level, never precise.
  const cityOf = useCallback(
    (n: NodeVisDto, i: number) => {
      let h = i * 7;
      for (const c of n.node_id) h = (h * 33 + c.charCodeAt(0)) >>> 0;
      return CITIES[h % CITIES.length];
    },
    [],
  );

  // shared pin position for draw + fly-to — announced coarse position wins;
  // sim nodes get a deterministic city; real nodes without an announced
  // position get no pin (we don't invent locations)
  const pinLL = useCallback(
    (t: NodeVisDto[], i: number): readonly [number, number] | null => {
      const n = t[i];
      if (n.lat != null && n.lon != null) return [n.lat, n.lon];
      if (real) return null;
      const c = cityOf(n, i);
      const dupes = t.slice(0, i).filter((o, j) => cityOf(o, j) === c).length;
      return [c[2] + dupes * 2.5, c[3] + dupes * 2.5] as const;
    },
    [cityOf, real],
  );

  const flyTo = (lat: number, lon: number, z: number) => {
    const v = ll2v(lat, lon);
    view.current.fly = {
      // yaw/pitch that brings v to the view centre (x→0, y→0, z→1)
      yaw: Math.atan2(-v[0], v[2]),
      pitch: Math.asin(Math.max(-1, Math.min(1, v[1]))),
      zoom: z,
    };
  };

  const draw = useCallback(() => {
    const c = ref.current;
    const el = wrap.current;
    if (!c || !el) return;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    const { width: w, height: h } = el.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    if (c.width !== w * dpr || c.height !== h * dpr) {
      c.width = w * dpr;
      c.height = h * dpr;
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    ctx.font = "11px ui-monospace, Menlo, monospace";
    ctx.textBaseline = "middle";

    // fly-to easing (shortest angular path for yaw)
    const f = view.current.fly;
    if (f) {
      const dy = Math.atan2(Math.sin(f.yaw - view.current.yaw), Math.cos(f.yaw - view.current.yaw));
      view.current.yaw += dy * 0.12;
      view.current.pitch += (f.pitch - view.current.pitch) * 0.12;
      view.current.zoom += (f.zoom - view.current.zoom) * 0.12;
      if (Math.abs(dy) < 0.003 && Math.abs(f.pitch - view.current.pitch) < 0.003 && Math.abs(f.zoom - view.current.zoom) < 0.02) {
        view.current.yaw = f.yaw;
        view.current.pitch = f.pitch;
        view.current.zoom = f.zoom;
        view.current.fly = null;
      }
    }

    const { yaw, pitch, zoom } = view.current;
    const cx = w / 2;
    const cy = h / 2;
    const R = Math.min(w, h) * 0.42;
    const topo = topoRef.current;
    const sel = selRef.current;
    const meI = meRef.current;

    // rotation matrix M = Rx(pitch)·Ry(yaw), applied to precomputed vectors
    const cyw = Math.cos(yaw), syw = Math.sin(yaw);
    const cp = Math.cos(pitch), sp = Math.sin(pitch);
    const m00 = cyw, m02 = syw;
    const m10 = sp * syw, m11 = cp, m12 = -sp * cyw;
    const m20 = -cp * syw, m21 = sp, m22 = cp * cyw;
    const apply = ([vx, vy, vz]: V3): V3 => [
      m00 * vx + m02 * vz,
      m10 * vx + m11 * vy + m12 * vz,
      m20 * vx + m21 * vy + m22 * vz,
    ];
    // zoom magnifies the whole sphere — the limb circle grows with it,
    // so at high zoom the horizon curves across the window (google-earth style)
    const RZ = R * zoom;
    const inView = (sx: number, sy: number) =>
      sx > -16 && sx < w + 16 && sy > -16 && sy < h + 16;

    // ---- land cells ----
    ctx.fillStyle = "rgba(255,255,255,0.30)";
    for (const v of LANDV) {
      const [x, y, z] = apply(v);
      if (z <= 0) continue;
      // screen-x is negated so longitude reads west→east correctly
      const sx = cx - x * RZ - 2;
      const sy = cy - y * RZ;
      if (!inView(sx, sy)) continue;
      ctx.fillText("·", sx, sy);
    }

    // ---- world cities: dots only when zoomed in (speckle noise at 1x) ----
    if (zoom >= 1.5) {
      for (const c of WCITIES) {
        const [x, y, z] = apply(c.v);
        if (z <= 0) continue;
        const sx = cx - x * RZ - 2;
        const sy = cy - y * RZ;
        if (!inView(sx, sy)) continue;
        ctx.fillStyle = c.pop >= 1_000_000 ? "rgba(255,255,255,0.4)" : "rgba(255,255,255,0.22)";
        ctx.fillText(c.pop >= 1_000_000 ? "+" : "·", sx, sy);
      }
    }

    // ---- limb circle + cardinal ticks ----
    ctx.strokeStyle = "rgba(255,255,255,0.18)";
    ctx.beginPath();
    ctx.arc(cx, cy, RZ + 6, 0, Math.PI * 2);
    ctx.stroke();
    ctx.fillStyle = "rgba(255,255,255,0.35)";
    ctx.fillText("+", cx - 2, cy - RZ - 12);
    ctx.fillText("+", cx - 2, cy + RZ + 4);
    ctx.fillText("+", cx - RZ - 10, cy);
    ctx.fillText("+", cx + RZ + 4, cy);

    // ---- node positions (coarse pins; null = position unannounced) ----
    const pos = topo.map((_, i) => {
      const ll = pinLL(topo, i);
      if (!ll) return null;
      const v = ll2v(ll[0], ll[1]);
      const [x, y, z] = apply(v);
      return { v, x, y, z };
    });

    // ---- links: great-circle dot trails + flow pulses ----
    const now = performance.now() / 1000;
    ctx.fillStyle = "rgba(255,255,255,0.75)";
    topo.forEach((n, i) => {
      for (const [peer] of n.heard) {
        if (peer <= i || !topo[peer]) continue; // each pair once
        if (!pos[i] || !pos[peer]) continue; // both ends need a position
        const a = pos[i]!.v;
        const b = pos[peer]!.v;
        const steps = 26;
        for (let s = 0; s <= steps; s++) {
          const p = slerp(a, b, s / steps);
          // lift the arc slightly for legibility
          const lift = 1 + 0.12 * Math.sin((s / steps) * Math.PI);
          const [x, y, z] = apply([p[0] * lift, p[1] * lift, p[2] * lift]);
          if (z <= 0.02) continue;
          const sx2 = cx - x * RZ - 2;
          const sy2 = cy - y * RZ;
          if (!inView(sx2, sy2)) continue;
          ctx.fillText(s % 2 ? ":" : "·", sx2, sy2);
        }
        // moving pulse along the link — packet flow direction a→b
        const ph = (now * 0.45 + (i * 0.37 + peer * 0.17)) % 1;
        const p = slerp(a, b, ph);
        const lift = 1 + 0.12 * Math.sin(ph * Math.PI);
        const [px, py, pz] = apply([p[0] * lift, p[1] * lift, p[2] * lift]);
        if (pz > 0.02) {
          const sx = cx - px * RZ;
          const sy = cy - py * RZ;
          if (inView(sx, sy)) {
            ctx.fillStyle = "#ffffff";
            ctx.fillText("*", sx - 2, sy);
            ctx.fillStyle = "rgba(255,255,255,0.75)";
          }
        }
      }
    });

    // ---- labels: collision-reserved rects, no overlapping text ever ----
    const placed: { x: number; y: number; w: number; h: number }[] = [];
    const collides = (x: number, y: number, w2: number, h2: number) =>
      placed.some(
        (p) => x < p.x + p.w && p.x < x + w2 && y < p.y + p.h && p.y < y + h2,
      );
    const tryBlock = (lines: string[], x: number, y: number, w2: number, style: string) => {
      const h2 = lines.length * 10 + 2;
      if (collides(x - 2, y - 6, w2 + 4, h2)) return false;
      placed.push({ x: x - 2, y: y - 6, w: w2 + 4, h: h2 });
      ctx.fillStyle = style;
      lines.forEach((l, k) => ctx.fillText(l, x, y + k * 10));
      return true;
    };

    // ---- pins + labels ----
    pins.current = [];
    topo.forEach((n, i) => {
      const p = pos[i];
      if (!p) return; // position unannounced — no pin
      const sx = cx - p.x * RZ;
      const sy = cy - p.y * RZ;
      const front = p.z > 0 && inView(sx, sy);
      const [city, country] = n.lat != null
        ? placeOf(n.lat, n.lon!)
        : cityOf(n, i);
      if (front) {
        pins.current.push({ i, sx, sy });
        const glyph = !n.enabled ? "x" : i === meI ? "@" : "o";
        ctx.fillStyle = i === sel ? "#ffffff" : n.enabled ? "#e6e6e6" : "#666666";
        ctx.font = "bold 13px ui-monospace, Menlo, monospace";
        ctx.fillText(glyph, sx - 4, sy - 4);
        ctx.font = "10px ui-monospace, Menlo, monospace";
        // try label spots around the pin; skip entirely if all collide
        const lines = [`${n.callsign}`, `${city}, ${country}`];
        const wmax = Math.max(...lines.map((l) => ctx.measureText(l).width));
        const spots: [number, number][] = [
          [sx + 8, sy - 8],
          [sx + 8, sy + 6],
          [sx - 12 - wmax, sy - 8],
          [sx - 12 - wmax, sy + 6],
        ];
        const style = i === sel ? "#ffffff" : "rgba(255,255,255,0.55)";
        for (const [tx, ty] of spots) if (tryBlock(lines, tx, ty, wmax, style)) break;
        if (i === sel) {
          // expanding ripple rings
          for (let k = 0; k < 2; k++) {
            const ph = (now * 0.9 + k * 0.5) % 1;
            ctx.strokeStyle = `rgba(255,255,255,${(0.55 * (1 - ph)).toFixed(2)})`;
            ctx.beginPath();
            ctx.arc(sx, sy, 8 + ph * 26, 0, Math.PI * 2);
            ctx.stroke();
          }
          // symbolic range ring (sim range is in metres — drawn at fixed radius)
          ctx.setLineDash([3, 4]);
          ctx.lineDashOffset = -now * 14;
          ctx.strokeStyle = "rgba(255,255,255,0.4)";
          ctx.beginPath();
          ctx.arc(sx, sy, 38, 0, Math.PI * 2);
          ctx.stroke();
          ctx.setLineDash([]);
        }
        if (i === meI) {
          // subtle blinking marker under the operating node
          ctx.fillStyle = `rgba(255,255,255,${(0.35 + 0.35 * Math.abs(Math.sin(now * 3))).toFixed(2)})`;
          ctx.fillText("+", sx - 2, sy + 9);
        }
      }
    });

    // ---- major country labels (lowest priority — skipped on collision) ----
    ctx.font = "10px ui-monospace, Menlo, monospace";
    for (const [name, v] of COUNTRYV) {
      const [x, y, z] = apply(v);
      if (z <= 0.05) continue;
      const sx = cx - x * RZ;
      const sy = cy - y * RZ;
      if (!inView(sx, sy)) continue;
      const w2 = ctx.measureText(name).width;
      tryBlock([name], sx - w2 / 2, sy, w2, "rgba(255,255,255,0.45)");
    }

    // ---- zoom readout ----
    if (zoom > 1.01) {
      ctx.fillStyle = "rgba(255,255,255,0.4)";
      ctx.fillText(`${zoom.toFixed(1)}x`, w - 40, 14);
    }

    // spin when idle (paused while zoomed or flying)
    if (!view.current.drag && !view.current.fly && zoom <= 1.01) view.current.yaw += 0.0018;
  }, [pinLL]);

  // render loop
  useEffect(() => {
    let raf = 0;
    const loop = () => {
      draw();
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [draw]);

  // pointer interaction
  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const down = (e: PointerEvent) => {
      view.current.drag = true;
      view.current.fly = null; // grab cancels an in-flight fly-to
      view.current.moved = 0;
      view.current.lx = e.clientX;
      view.current.ly = e.clientY;
      el.setPointerCapture(e.pointerId);
    };
    const move = (e: PointerEvent) => {
      if (!view.current.drag) return;
      const dx = e.clientX - view.current.lx;
      const dy = e.clientY - view.current.ly;
      view.current.moved += Math.abs(dx) + Math.abs(dy);
      view.current.lx = e.clientX;
      view.current.ly = e.clientY;
      // grab-the-globe: the point under the cursor follows the cursor
      // sensitivity drops with zoom so close-up pans stay controllable
      const k = 0.008 / view.current.zoom;
      view.current.yaw -= dx * k;
      view.current.pitch = Math.max(-1.35, Math.min(1.35, view.current.pitch + dy * k));
    };
    const up = (e: PointerEvent) => {
      view.current.drag = false;
      if (view.current.moved < 4) {
        // click → select nearest pin
        const r = el.getBoundingClientRect();
        const px = e.clientX - r.left;
        const py = e.clientY - r.top;
        let best: number | null = null;
        let bd = 32;
        for (const p of pins.current) {
          const d = Math.hypot(p.sx - px, p.sy - py);
          if (d < bd) {
            bd = d;
            best = p.i;
          }
        }
        setSelected(best);
      }
    };
    const wheel = (e: WheelEvent) => {
      e.preventDefault();
      const z = view.current.zoom * Math.exp(-e.deltaY * 0.0015);
      view.current.zoom = Math.max(1, Math.min(8, z));
    };
    const dbl = () => {
      view.current.zoom = 1;
      view.current.fly = null;
    };
    el.addEventListener("pointerdown", down);
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("wheel", wheel, { passive: false });
    el.addEventListener("dblclick", dbl);
    return () => {
      el.removeEventListener("pointerdown", down);
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("wheel", wheel);
      el.removeEventListener("dblclick", dbl);
    };
  }, []);

  // ---- find: node callsign / id prefix, or city / country ----
  const [query, setQuery] = useState("");
  const [miss, setMiss] = useState(false);
  const [qActive, setQActive] = useState(-1);

  const suggestions = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    const out: { kind: "node" | "city"; label: string; run: () => void }[] = [];
    topo.forEach((n, i) => {
      if (n.callsign.toLowerCase().includes(q) || n.node_id.toLowerCase().startsWith(q)) {
        const [city, country] = cityOf(n, i);
        out.push({
          kind: "node",
          label: `${n.callsign} · ${city}, ${country}`,
          run: () => {
            const ll = pinLL(topoRef.current, i);
            if (ll) flyTo(ll[0], ll[1], 3);
            setSelected(i);
          },
        });
      }
    });
    CITIES.forEach((c) => {
      if (c[0].includes(q) || c[1].includes(q))
        out.push({
          kind: "city",
          label: `${c[0]}, ${c[1]}`,
          run: () => flyTo(c[2], c[3], 3),
        });
    });
    // every world city too
    for (const c of WCITIES) {
      if (out.length >= 9) break;
      if (c.name.includes(q))
        out.push({
          kind: "city",
          label: `${c.name}, ${c.cc}`,
          run: () => flyTo(c.lat, c.lon, 3),
        });
    }
    return out.slice(0, 9);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, topo, cityOf, pinLL]);

  const find = (e: React.FormEvent) => {
    e.preventDefault();
    if (qActive >= 0 && suggestions[qActive]) {
      suggestions[qActive].run();
      setQActive(-1);
      return;
    }
    const q = query.trim().toLowerCase();
    if (!q) return;
    if (suggestions[0]) {
      suggestions[0].run();
      return;
    }
    setMiss(true);
    setTimeout(() => setMiss(false), 1500);
  };

  const onFindKey = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setQActive((a) => Math.min(a + 1, suggestions.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setQActive((a) => Math.max(a - 1, -1));
    } else if (e.key === "Escape") {
      setQuery("");
      setQActive(-1);
    }
  };

  const sel = selected !== null ? topo[selected] : null;
  const selCity = sel
    ? sel.lat != null
      ? placeOf(sel.lat, sel.lon!)
      : real
        ? null
        : cityOf(sel, selected!)
    : null;
  // estimated RF range from node spacing (medium uses 1.6x spacing)
  const rangeM =
    topo.length > 1
      ? Math.round(Math.hypot(topo[1].x - topo[0].x, topo[1].y - topo[0].y) * 1.6)
      : 0;

  return (
    <div className="flex h-full flex-col gap-3">
      <div className="relative min-h-0 flex-1 border border-line bg-panel">
        <div ref={wrap} className="absolute inset-0 cursor-grab touch-none active:cursor-grabbing">
          <canvas ref={ref} className="absolute inset-0 h-full w-full" />
        </div>
        <div className="pointer-events-none absolute left-3 top-2 text-dim">
          ucr://net — drag to rotate · scroll to zoom · dblclick reset ·{" "}
          {real ? "peers heard on the wire · city-level announced positions" : "coarse city-level positions"}
        </div>
        <form className="absolute left-3 top-8" onSubmit={find}>
          <div className="flex items-center gap-2">
            <span className="text-dim">&gt;</span>
            <input
              value={query}
              onChange={(e) => {
                setQuery(e.target.value);
                setQActive(-1);
              }}
              onKeyDown={onFindKey}
              placeholder="find node / place…"
              className="w-48 border border-line bg-panel px-2 py-0.5 text-fg placeholder:text-dim focus:outline-none"
            />
            {miss && <span className="text-dim">no match</span>}
          </div>
          {suggestions.length > 0 && (
            <div className="mt-1 w-64 border border-line bg-panel">
              {suggestions.map((s, si) => (
                <button
                  type="button"
                  key={si}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    s.run();
                    setQuery("");
                    setQActive(-1);
                  }}
                  onMouseEnter={() => setQActive(si)}
                  className={`block w-full px-2 py-1 text-left ${
                    si === qActive ? "bg-panel-2 text-fg" : "text-dim hover:text-fg"
                  }`}
                >
                  {s.kind === "node" ? "o" : "+"} {s.label}
                </button>
              ))}
            </div>
          )}
        </form>
      </div>
      <div className="flex items-center gap-4 border border-line bg-panel px-4 py-2 text-dim">
        {sel ? (
          <>
            <span className="text-fg">[{sel.callsign}]</span>
            <span>
              {selCity
                ? `${selCity[0]}, ${selCity[1]}`
                : "position unannounced"}
            </span>
            <span>{sel.node_id.slice(0, 16)}</span>
            <span>{sel.enabled ? "radio on" : "radio off"}</span>
            {rangeM > 0 && <span>range ~{rangeM}m</span>}
            <span>
              heard:{" "}
              {sel.heard.length
                ? sel.heard
                    .map(([p, r]) =>
                      r <= -90
                        ? `${topo[p]?.callsign ?? `n${p}`} ip`
                        : `${topo[p]?.callsign ?? `n${p}`} ${Math.round(r)}dBm`,
                    )
                    .join(" · ")
                : "none"}
            </span>
            {real ? null : (
              <button
                onClick={() => onSetEnabled(sel.id, !sel.enabled)}
              className="ml-auto border border-line bg-panel-2 px-3 py-1 text-fg hover:bg-fg hover:text-ink"
              >
                {sel.enabled ? "[ disable ]" : "[ enable ]"}
              </button>
            )}
          </>
        ) : (
          <span>click a pin — dotted trails are heard peer links · pulses show traffic flow</span>
        )}
        <span className="ml-auto">{topo.length} nodes</span>
      </div>
    </div>
  );
}
