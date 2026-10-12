import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// ---- types ---------------------------------------------------------------

export interface NodeVisDto {
  id: number;
  x: number;
  y: number;
  enabled: boolean;
  node_id: string;
  callsign: string;
  heard: [number, number][];
}

export interface NeighborDto {
  node_id: string;
  callsign: string;
  rssi_ewma: number;
  snr_ewma: number;
  last_seen_ms: number;
  rx_count: number;
  bidirectional: boolean;
  quality: number;
}

export interface RouteDto {
  dest: string;
  next_hop: string;
  metric: number;
  hops: number;
}

export interface StatsDto {
  medium_tx: number;
  medium_delivered: number;
  medium_dropped_loss: number;
  medium_dropped_collision: number;
  medium_dropped_halfduplex: number;
  medium_dropped_blocked: number;
  airtime_ms: number;
}

export interface NodeInfoDto {
  node_id: string;
  callsign: string;
  pubkey: string;
}

export interface ChannelDto {
  id: string;
  name: string;
  category: string;
  scope: string;
}

export interface ContactDto {
  node_id: string;
  callsign: string;
  desc: string;
  blocked: boolean;
  trusted: boolean;
  verified: boolean;
  favourite: boolean;
}

export interface GroupDto {
  group_id: string;
  name: string;
  desc: string;
  epoch: number;
  members: number;
}

export interface StoredMsg {
  msg_id: number;
  peer: string;
  direction: string;
  text: string;
  at_ms: number;
  delivery: string;
}

export interface UcrEvent {
  node: number;
  event: Record<string, unknown>;
}

export function eventKind(e: Record<string, unknown>): string {
  const keys = Object.keys(e);
  return keys.length ? keys[0] : "Unknown";
}

export function eventBody(e: Record<string, unknown>): Record<string, unknown> {
  const k = eventKind(e);
  const v = e[k];
  return (v && typeof v === "object" ? v : {}) as Record<string, unknown>;
}

// ---- commands ------------------------------------------------------------

export const api = {
  simStart: (count: number, spacing_m: number, seed: number) =>
    invoke<void>("sim_start", { count, spacingM: spacing_m, seed }),
  simStop: () => invoke<void>("sim_stop"),
  simTopology: () => invoke<NodeVisDto[]>("sim_topology"),
  simStats: () => invoke<StatsDto>("sim_stats"),
  simSetEnabled: (idx: number, enabled: boolean) =>
    invoke<void>("sim_set_enabled", { idx, enabled }),

  nodeInfo: (idx: number) => invoke<NodeInfoDto>("node_info", { idx }),
  nodeNeighbors: (idx: number) => invoke<NeighborDto[]>("node_neighbors", { idx }),
  nodeRoutes: (idx: number) => invoke<RouteDto[]>("node_routes", { idx }),
  nodeChannels: (idx: number) => invoke<ChannelDto[]>("node_channels", { idx }),
  nodeContacts: (idx: number) => invoke<ContactDto[]>("node_contacts", { idx }),
  nodeGroups: (idx: number) => invoke<GroupDto[]>("node_groups", { idx }),
  nodeDmHistory: (idx: number, peer: string) =>
    invoke<StoredMsg[]>("node_dm_history", { idx, peer }),
  nodeChannelHistory: (idx: number, channel: string) =>
    invoke<StoredMsg[]>("node_channel_history", { idx, channel }),
  nodeStats: (idx: number) => invoke<{ router: string }>("node_stats", { idx }),

  joinChannel: (idx: number, name: string) => invoke<string>("join_channel", { idx, name }),
  sendChannelText: (idx: number, channel: string, text: string) =>
    invoke<number>("send_channel_text", { idx, channel, text }),
  sendDm: (idx: number, peer: string, text: string) =>
    invoke<number>("send_dm", { idx, peer, text }),
  sendMorse: (idx: number, peer: string, text: string, wpm: number) =>
    invoke<number>("send_morse", { idx, peer, text, wpm }),
  sendEmergency: (idx: number, category: string, text: string) =>
    invoke<number>("send_emergency", { idx, category, text }),
  createGroup: (idx: number, name: string, desc: string) =>
    invoke<string>("create_group", { idx, name, desc }),
  inviteGroup: (idx: number, groupId: string, peer: string) =>
    invoke<void>("invite_group", { idx, groupId, peer }),
  acceptGroup: (idx: number, groupId: string) =>
    invoke<void>("accept_group", { idx, groupId }),
  sendGroupText: (idx: number, groupId: string, text: string) =>
    invoke<number>("send_group_text", { idx, groupId, text }),
  acceptContact: (idx: number, peer: string) =>
    invoke<void>("accept_contact", { idx, peer }),
  blockContact: (idx: number, peer: string) =>
    invoke<void>("block_contact", { idx, peer }),
  subscribeEvents: () => invoke<void>("subscribe_events"),
};

export const onUcrEvent = (cb: (e: UcrEvent) => void) =>
  listen<UcrEvent>("ucr:event", (ev) => cb(ev.payload));
