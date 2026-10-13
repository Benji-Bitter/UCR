// Browser-side fallback simulator — used when the Tauri backend is absent
// (e.g. `pnpm dev` in a plain browser). Ports the same semantics as the
// Rust SimWorld: line topology, range = 1.6×spacing (adjacent hearing only),
// BFS multi-hop paths, flood vs unicast delivery, store-and-forward retry.

import type {
  ChannelDto,
  ContactDto,
  GroupDto,
  NeighborDto,
  NodeInfoDto,
  NodeVisDto,
  RouteDto,
  StatsDto,
  StoredMsg,
  UcrEvent,
} from "./api";

type Ev = Record<string, unknown>;
type Cb = (e: UcrEvent) => void;

function mulberry32(seed: number) {
  let a = seed >>> 0;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const hex = (rng: () => number, n: number) =>
  Array.from({ length: n }, () => Math.floor(rng() * 16).toString(16)).join("");

const fnv8 = (s: string) => {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return (h >>> 0).toString(16).padStart(8, "0");
};

const DEFAULT_CHANNELS: [string, string, string][] = [
  ["ucr-general", "General", "Regional"],
  ["ucr-local", "Local", "Local"],
  ["marine", "Marine", "Regional"],
  ["roadwatch", "RoadTransport", "Regional"],
  ["community", "Community", "Local"],
  ["tech-talk", "Technical", "Extended"],
  ["emergency-info", "EmergencyInfo", "Extended"],
];

const CHATTER = [
  "cq cq cq de {cs} k",
  "signal report please {cs}",
  "mesh looking good from this end",
  "anyone else hearing node 0?",
  "73 de {cs}",
  "testing relay path, ack pls",
];

interface Contact {
  node_id: string;
  callsign: string;
  desc: string;
  blocked: boolean;
  trusted: boolean;
  verified: boolean;
  favourite: boolean;
}

interface Group {
  group_id: string;
  name: string;
  desc: string;
  epoch: number;
  members: Set<number>;
}

interface Pending {
  kind: "dm" | "morse" | "invite" | "group";
  from: number;
  to: number;
  text: string;
  tries: number;
  extra?: string | number;
}

export class FallbackSim {
  private rng = mulberry32(1);
  private listeners = new Set<Cb>();
  private timers = new Set<ReturnType<typeof setTimeout>>();
  private nodes: {
    node_id: string;
    pubkey: string;
    callsign: string;
    x: number;
    y: number;
    enabled: boolean;
    channels: Set<string>;
    contacts: Map<string, Contact>;
    rxCount: Map<number, number>;
  }[] = [];
  private groups = new Map<string, Group>();
  private pendingInvites: { group_id: string; node: number }[] = [];
  private dmHist = new Map<string, StoredMsg[]>(); // `${idx}|${peer}`
  private chHist = new Map<string, StoredMsg[]>(); // `${idx}|${channel}`
  private queue: Pending[] = [];
  private stats: StatsDto = {
    medium_tx: 0,
    medium_delivered: 0,
    medium_dropped_loss: 0,
    medium_dropped_collision: 0,
    medium_dropped_halfduplex: 0,
    medium_dropped_blocked: 0,
    airtime_ms: 0,
    link_mtu: 400,
    link_bitrate: 5500,
    link_name: "simulator",
  };
  private msgSeq = 1;
  private pktSeq = 1;
  private sessions = new Set<string>(); // `${a}|${b}` established
  private chatterTimer: ReturnType<typeof setInterval> | null = null;
  private queueTimer: ReturnType<typeof setInterval> | null = null;

  // ---- lifecycle -------------------------------------------------------

  simStart(count: number, spacingM: number, seed: number): Promise<void> {
    this.simStop();
    this.rng = mulberry32(seed);
    const n = Math.max(2, Math.min(12, count));
    this.nodes = Array.from({ length: n }, (_, i) => ({
      node_id: hex(this.rng, 64),
      pubkey: hex(this.rng, 64),
      callsign: `SIM-${String(i).padStart(2, "0")}`,
      x: i * spacingM,
      y: 0,
      enabled: true,
      channels: new Set(DEFAULT_CHANNELS.map(([name]) => name)),
      contacts: new Map(),
      rxCount: new Map(),
    }));
    this.spacing = spacingM;
    // occasional ambient traffic so the mesh feels alive
    this.chatterTimer = setInterval(() => {
      if (!this.nodes.length || this.rng() > 0.4) return;
      const i = Math.floor(this.rng() * this.nodes.length);
      if (!this.nodes[i].enabled) return;
      const text = CHATTER[Math.floor(this.rng() * CHATTER.length)].replace(
        "{cs}",
        this.nodes[i].callsign,
      );
      this.floodText(i, "ucr-general", text);
    }, 22000);
    this.queueTimer = setInterval(() => this.flushQueue(), 1500);
    return Promise.resolve();
  }
  private spacing = 300;

  simStop(): Promise<void> {
    for (const t of this.timers) clearTimeout(t);
    this.timers.clear();
    if (this.chatterTimer) clearInterval(this.chatterTimer);
    if (this.queueTimer) clearInterval(this.queueTimer);
    this.chatterTimer = this.queueTimer = null;
    this.nodes = [];
    this.groups.clear();
    this.pendingInvites = [];
    this.queue = [];
    this.sessions.clear();
    this.dmHist.clear();
    this.chHist.clear();
    this.stats = {
      medium_tx: 0, medium_delivered: 0, medium_dropped_loss: 0,
      medium_dropped_collision: 0, medium_dropped_halfduplex: 0,
      medium_dropped_blocked: 0, airtime_ms: 0,
      link_mtu: 400, link_bitrate: 5500, link_name: "simulator",
    };
    return Promise.resolve();
  }

  simSetEnabled(idx: number, enabled: boolean): Promise<void> {
    const n = this.nodes[idx];
    if (n) {
      n.enabled = enabled;
      this.emitAll({ NeighborUpdate: {} });
    }
    return Promise.resolve();
  }

  // ---- graph -----------------------------------------------------------

  private pathHops(from: number, to: number): number | null {
    // BFS over adjacent links among enabled nodes (line topology, but BFS
    // stays honest if the topology ever changes).
    if (from === to) return 0;
    const dist = new Map<number, number>([[from, 0]]);
    const q = [from];
    while (q.length) {
      const cur = q.shift()!;
      const d = dist.get(cur)!;
      for (const nb of [cur - 1, cur + 1]) {
        if (nb < 0 || nb >= this.nodes.length) continue;
        if (!this.nodes[nb].enabled || dist.has(nb)) continue;
        if (nb === to) return d + 1;
        dist.set(nb, d + 1);
        q.push(nb);
      }
    }
    return null;
  }

  private rssi(dist: number): number {
    // free-space-ish: −40 − 20·log10(d) + jitter
    return -40 - 20 * Math.log10(Math.max(1, dist)) + (this.rng() - 0.5) * 4;
  }

  simTopology(): Promise<NodeVisDto[]> {
    const out = this.nodes.map((n, i) => {
      const heard: [number, number][] = [];
      if (n.enabled) {
        for (const j of [i - 1, i + 1]) {
          const m = this.nodes[j];
          if (m && m.enabled) heard.push([j, this.rssi(this.spacing)]);
        }
      }
      return {
        id: i,
        x: n.x,
        y: n.y,
        enabled: n.enabled,
        node_id: n.node_id,
        callsign: n.callsign,
        heard,
      };
    });
    return Promise.resolve(out);
  }

  simStats(): Promise<StatsDto> {
    return Promise.resolve({ ...this.stats });
  }

  // ---- node queries ------------------------------------------------------

  nodeInfo(idx: number): Promise<NodeInfoDto> {
    const n = this.nodes[idx];
    return Promise.resolve({
      node_id: n.node_id,
      callsign: n.callsign,
      pubkey: n.pubkey,
    });
  }

  nodeNeighbors(idx: number): Promise<NeighborDto[]> {
    const n = this.nodes[idx];
    if (!n?.enabled) return Promise.resolve([]);
    const out: NeighborDto[] = [];
    for (const j of [idx - 1, idx + 1]) {
      const m = this.nodes[j];
      if (!m?.enabled) continue;
      const rx = (n.rxCount.get(j) ?? 0) + 1;
      n.rxCount.set(j, rx);
      out.push({
        node_id: m.node_id,
        callsign: m.callsign,
        rssi_ewma: this.rssi(this.spacing),
        snr_ewma: 8 + this.rng() * 4,
        last_seen_ms: Math.floor(this.rng() * 1200),
        rx_count: rx,
        bidirectional: true,
        quality: 0.85 + this.rng() * 0.15,
      });
    }
    return Promise.resolve(out);
  }

  nodeRoutes(idx: number): Promise<RouteDto[]> {
    const out: RouteDto[] = [];
    for (let j = 0; j < this.nodes.length; j++) {
      if (j === idx) continue;
      const hops = this.pathHops(idx, j);
      if (hops === null || hops === 0) continue;
      out.push({
        dest: this.nodes[j].node_id,
        next_hop: this.nodes[idx + Math.sign(j - idx)].node_id,
        metric: hops * 10,
        hops,
      });
    }
    return Promise.resolve(out);
  }

  nodeChannels(idx: number): Promise<ChannelDto[]> {
    const n = this.nodes[idx];
    const out = [...n.channels].map((name) => ({
      id: fnv8(name),
      name,
      category:
        DEFAULT_CHANNELS.find(([d]) => d === name)?.[1] ?? "Custom",
      scope: DEFAULT_CHANNELS.find(([d]) => d === name)?.[2] ?? "Local",
    }));
    return Promise.resolve(out);
  }

  nodeContacts(idx: number): Promise<ContactDto[]> {
    return Promise.resolve([...this.nodes[idx].contacts.values()]);
  }

  nodeGroups(idx: number): Promise<GroupDto[]> {
    const out: GroupDto[] = [];
    for (const g of this.groups.values())
      if (g.members.has(idx))
        out.push({
          group_id: g.group_id,
          name: g.name,
          desc: g.desc,
          epoch: g.epoch,
          members: g.members.size,
        });
    return Promise.resolve(out);
  }

  nodeDmHistory(idx: number, peer: string): Promise<StoredMsg[]> {
    return Promise.resolve(this.dmHist.get(`${idx}|${peer}`) ?? []);
  }

  nodeChannelHistory(idx: number, channel: string): Promise<StoredMsg[]> {
    return Promise.resolve(this.chHist.get(`${idx}|${channel}`) ?? []);
  }

  nodeStats(idx: number): Promise<{ router: string }> {
    const nb = this.nodes[idx]?.enabled
      ? [idx - 1, idx + 1].filter(
          (j) => this.nodes[j]?.enabled,
        ).length
      : 0;
    const routes = this.nodes.filter(
      (_, j) => j !== idx && this.pathHops(idx, j) !== null,
    ).length;
    return Promise.resolve({
      router: `neighbors=${nb} routes=${routes} queue=${this.queue.length}`,
    });
  }

  // ---- sends -------------------------------------------------------------

  joinChannel(idx: number, name: string): Promise<string> {
    this.nodes[idx].channels.add(name);
    return Promise.resolve(fnv8(name));
  }

  sendChannelText(idx: number, channel: string, text: string): Promise<number> {
    const pid = this.pktSeq++;
    const msgId = this.msgSeq++;
    this.recordCh(idx, channel, this.nodes[idx].node_id, "Outbound", text, "Sent");
    this.emit(idx, { Delivery: { packet_id: pid, msg_id: msgId, peer: null, channel: fnv8(channel), state: "Sent" } });
    this.floodText(idx, channel, text, msgId, pid);
    return Promise.resolve(pid);
  }

  // Voice: echo the frame back as if the nearest sim peer relayed it — lets
  // the mic→codec→speaker path be exercised with no real peers.
  sendVoice(
    idx: number,
    channel: string,
    codec: number,
    txId: number,
    seq: number,
    last: boolean,
    data: number[],
  ): Promise<number> {
    const peer = this.nodes.find((n, i) => i !== idx && n.enabled) ?? this.nodes[idx];
    setTimeout(() => {
      this.emit(idx, {
        VoiceFrame: {
          channel: fnv8(channel),
          channel_name: channel,
          from: peer.node_id,
          callsign: peer.callsign,
          codec,
          tx_id: txId,
          seq,
          last,
          data,
          hops: 1,
        },
      });
    }, 60);
    return Promise.resolve(this.pktSeq++);
  }

  sendDm(idx: number, peer: string, text: string): Promise<number> {
    const pid = this.pktSeq++;
    const msgId = this.msgSeq++;
    const to = this.nodes.findIndex((n) => n.node_id === peer);
    this.emit(idx, { Delivery: { packet_id: pid, msg_id: msgId, peer, channel: null, state: "Sent" } });
    if (to < 0) {
      this.emit(idx, { Delivery: { packet_id: pid, msg_id: msgId, peer, channel: null, state: "Failed" } });
      return Promise.resolve(pid);
    }
    this.ensureContact(idx, to);
    this.ensureContact(to, idx);
    this.recordDm(idx, peer, "Outbound", text, "Sent");
    const hops = this.pathHops(idx, to);
    if (hops === null) {
      this.queue.push({ kind: "dm", from: idx, to, text, tries: 0, extra: msgId });
      this.recordDm(idx, peer, "Outbound", text, "Queued");
      return Promise.resolve(pid);
    }
    this.deliverDm(idx, to, text, msgId, pid, hops);
    return Promise.resolve(pid);
  }

  sendMorse(idx: number, peer: string, text: string, wpm: number): Promise<number> {
    const pid = this.pktSeq++;
    const to = this.nodes.findIndex((n) => n.node_id === peer);
    this.emit(idx, { Delivery: { packet_id: pid, msg_id: null, peer, channel: null, state: "Sent" } });
    if (to >= 0) {
      const hops = this.pathHops(idx, to);
      if (hops !== null) {
        this.after(hops * 140 + 80, () => {
          if (!this.nodes[to].enabled || this.nodes[to].contacts.get(this.nodes[idx].node_id)?.blocked) return;
          this.emit(to, {
            Morse: { from: this.nodes[idx].node_id, callsign: this.nodes[idx].callsign, text, wpm, at_ms: Date.now() },
          });
          this.emit(idx, { Delivery: { packet_id: pid, msg_id: null, peer, channel: null, state: "Delivered" } });
        });
      } else {
        this.queue.push({ kind: "morse", from: idx, to, text, tries: 0, extra: wpm });
      }
    }
    return Promise.resolve(pid);
  }

  sendEmergency(idx: number, category: string, text: string): Promise<number> {
    const pid = this.pktSeq++;
    this.emit(idx, { Delivery: { packet_id: pid, msg_id: null, peer: null, channel: null, state: "Sent" } });
    for (let j = 0; j < this.nodes.length; j++) {
      if (j === idx) continue;
      const hops = this.pathHops(idx, j);
      if (hops === null) continue;
      this.after(hops * 140 + 60, () => {
        if (!this.nodes[j].enabled) return;
        this.emit(j, {
          Emergency: {
            from: this.nodes[idx].node_id,
            callsign: this.nodes[idx].callsign,
            category,
            text,
            pos: null,
            emergency_id: pid,
            at_ms: Date.now(),
            hops,
            verified: true,
          },
        });
        this.stats.medium_delivered++;
      });
      this.tx(hops);
    }
    this.emit(idx, { Propagated: { packet_id: pid, hops: 1 } });
    return Promise.resolve(pid);
  }

  createGroup(idx: number, name: string, desc: string): Promise<string> {
    const group_id = hex(this.rng, 16);
    this.groups.set(group_id, {
      group_id, name, desc, epoch: 1, members: new Set([idx]),
    });
    return Promise.resolve(group_id);
  }

  inviteGroup(idx: number, groupId: string, peer: string): Promise<void> {
    const g = this.groups.get(groupId);
    const to = this.nodes.findIndex((n) => n.node_id === peer);
    if (!g || to < 0) return Promise.resolve();
    const hops = this.pathHops(idx, to);
    if (hops === null) {
      this.queue.push({ kind: "invite", from: idx, to, text: "", tries: 0, extra: groupId });
      return Promise.resolve();
    }
    this.after(hops * 140 + 80, () => {
      this.pendingInvites.push({ group_id: groupId, node: to });
      this.emit(to, {
        GroupInvite: {
          group_id: groupId,
          name: g.name,
          desc: g.desc,
          from: this.nodes[idx].node_id,
          callsign: this.nodes[idx].callsign,
        },
      });
    });
    this.tx(hops);
    return Promise.resolve();
  }

  acceptGroup(idx: number, groupId: string): Promise<void> {
    const g = this.groups.get(groupId);
    if (!g || g.members.has(idx)) return Promise.resolve();
    this.pendingInvites = this.pendingInvites.filter(
      (p) => !(p.group_id === groupId && p.node === idx),
    );
    g.members.add(idx);
    g.epoch++;
    const roster = [...g.members].map((i) => this.nodes[i].node_id);
    for (const m of g.members) {
      this.emit(m, { GroupEpoch: { group_id: groupId, epoch: g.epoch } });
      this.emit(m, { GroupRoster: { group_id: groupId, members: roster } });
    }
    return Promise.resolve();
  }

  sendGroupText(idx: number, groupId: string, text: string): Promise<number> {
    const pid = this.pktSeq++;
    const msgId = this.msgSeq++;
    const g = this.groups.get(groupId);
    if (!g) return Promise.resolve(pid);
    this.emit(idx, { Delivery: { packet_id: pid, msg_id: msgId, peer: null, channel: null, state: "Sent" } });
    for (const m of g.members) {
      if (m === idx) continue;
      const hops = this.pathHops(idx, m);
      if (hops === null) continue;
      this.after(hops * 140 + 80, () => {
        this.emit(m, {
          GroupMessage: {
            group_id: groupId,
            from: this.nodes[idx].node_id,
            callsign: this.nodes[idx].callsign,
            text,
            msg_id: msgId,
            at_ms: Date.now(),
          },
        });
        this.stats.medium_delivered++;
      });
      this.tx(hops);
    }
    return Promise.resolve(pid);
  }

  acceptContact(idx: number, peer: string): Promise<void> {
    const c = this.nodes[idx].contacts.get(peer);
    if (c) {
      c.trusted = true;
      c.verified = true;
    }
    return Promise.resolve();
  }

  blockContact(idx: number, peer: string): Promise<void> {
    const c = this.nodes[idx].contacts.get(peer);
    if (c) c.blocked = true;
    return Promise.resolve();
  }

  subscribeEvents(): Promise<void> {
    return Promise.resolve();
  }

  onEvent(cb: Cb): Promise<() => void> {
    this.listeners.add(cb);
    return Promise.resolve(() => this.listeners.delete(cb));
  }

  // ---- internals ---------------------------------------------------------

  private after(ms: number, f: () => void) {
    const t = setTimeout(() => {
      this.timers.delete(t);
      f();
    }, ms);
    this.timers.add(t);
  }

  private emit(node: number, event: Ev) {
    const e: UcrEvent = { node, event };
    for (const cb of this.listeners) cb(e);
  }

  private emitAll(event: Ev) {
    for (let i = 0; i < this.nodes.length; i++) this.emit(i, event);
  }

  private tx(hops: number) {
    this.stats.medium_tx += hops;
    this.stats.airtime_ms += hops * 160;
    if (this.rng() < 0.03) this.stats.medium_dropped_collision++;
  }

  private floodText(idx: number, channel: string, text: string, msgId?: number, pid?: number) {
    const mid = msgId ?? this.msgSeq++;
    let reached = false;
    for (let j = 0; j < this.nodes.length; j++) {
      if (j === idx) continue;
      const hops = this.pathHops(idx, j);
      if (hops === null) continue;
      reached = true;
      this.after(hops * 140 + 60 + this.rng() * 40, () => {
        const r = this.nodes[j];
        if (!r.enabled || !r.channels.has(channel)) return;
        if (r.contacts.get(this.nodes[idx].node_id)?.blocked) return;
        this.emit(j, {
          ChannelMessage: {
            channel: fnv8(channel),
            channel_name: channel,
            from: this.nodes[idx].node_id,
            callsign: this.nodes[idx].callsign,
            text,
            msg_id: mid,
            at_ms: Date.now(),
            hops,
            verified: true,
          },
        });
        this.recordCh(j, channel, this.nodes[idx].node_id, "Inbound", text, "Delivered");
        this.stats.medium_delivered++;
      });
      this.tx(hops);
    }
    if (reached && pid !== undefined)
      this.after(200, () => this.emit(idx, { Propagated: { packet_id: pid, hops: 1 } }));
  }

  private deliverDm(from: number, to: number, text: string, msgId: number, pid: number, hops: number) {
    const peer = this.nodes[to].node_id;
    const selfId = this.nodes[from].node_id;
    this.tx(hops);
    const key = `${Math.min(from, to)}|${Math.max(from, to)}`;
    this.after(hops * 140 + 60, () => {
      if (!this.sessions.has(key)) {
        this.sessions.add(key);
        this.emit(from, { SessionEstablished: { peer } });
        this.emit(to, { SessionEstablished: { peer: selfId } });
      }
    });
    this.after(hops * 140 + 160, () => {
      if (!this.nodes[to].enabled) {
        this.queue.push({ kind: "dm", from, to, text, tries: 0, extra: msgId });
        return;
      }
      if (this.nodes[to].contacts.get(selfId)?.blocked) {
        this.emit(from, { Delivery: { packet_id: pid, msg_id: msgId, peer, channel: null, state: "Failed" } });
        return;
      }
      this.stats.medium_delivered++;
      this.emit(to, {
        DirectMessage: { from: selfId, callsign: this.nodes[from].callsign, text, msg_id: msgId, at_ms: Date.now() },
      });
      this.recordDm(to, selfId, "Inbound", text, "Delivered");
      this.after(120, () =>
        this.emit(from, { Delivery: { packet_id: pid, msg_id: msgId, peer, channel: null, state: "Delivered" } }),
      );
    });
  }

  private flushQueue() {
    const keep: Pending[] = [];
    for (const p of this.queue) {
      const hops = this.pathHops(p.from, p.to);
      if (hops !== null && this.nodes[p.to].enabled) {
        if (p.kind === "dm") {
          this.deliverDm(p.from, p.to, p.text, Number(p.extra), this.pktSeq++, hops);
        } else if (p.kind === "morse") {
          void this.sendMorse(p.from, this.nodes[p.to].node_id, p.text, Number(p.extra));
        } else if (p.kind === "invite") {
          void this.inviteGroup(p.from, String(p.extra), this.nodes[p.to].node_id);
        }
      } else if (++p.tries < 8) {
        keep.push(p);
      } else {
        this.emit(p.from, {
          Delivery: { packet_id: this.pktSeq++, msg_id: Number(p.extra) || null, peer: this.nodes[p.to].node_id, channel: null, state: "Failed" },
        });
      }
    }
    this.queue = keep;
  }

  private ensureContact(idx: number, peerIdx: number) {
    const n = this.nodes[idx];
    const p = this.nodes[peerIdx];
    if (!n.contacts.has(p.node_id))
      n.contacts.set(p.node_id, {
        node_id: p.node_id,
        callsign: p.callsign,
        desc: "simulated node",
        blocked: false,
        trusted: false,
        verified: false,
        favourite: false,
      });
  }

  private recordDm(idx: number, peer: string, dir: string, text: string, delivery: string) {
    const k = `${idx}|${peer}`;
    const list = this.dmHist.get(k) ?? [];
    list.push({
      msg_id: this.msgSeq++,
      peer,
      direction: dir,
      text,
      at_ms: Date.now(),
      delivery,
    });
    this.dmHist.set(k, list.slice(-200));
  }

  private recordCh(idx: number, channel: string, peer: string, dir: string, text: string, delivery: string) {
    const k = `${idx}|${channel}`;
    const list = this.chHist.get(k) ?? [];
    list.push({
      msg_id: this.msgSeq++,
      peer,
      direction: dir,
      text,
      at_ms: Date.now(),
      delivery,
    });
    this.chHist.set(k, list.slice(-200));
  }
}
