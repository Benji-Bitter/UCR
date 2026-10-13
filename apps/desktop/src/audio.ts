// Voice engine: mic → IMA-ADPCM → UCR voice frames → speaker.
//
// Codec 1 = IMA-ADPCM 4-bit mono at 8kHz (~32 kbit/s). Chosen because it's
// pure arithmetic — no native codec dependency — and 100ms of audio fits
// one wire packet. Over LAN/TCP it is speech-quality; over real LoRa-class
// links Codec2 (codec id 2, firmware-side) is the bandwidth-honest codec.

export const VOICE_CODEC_ADPCM = 1;
export const VOICE_RATE = 8000;
// samples per frame — 100ms of 8kHz mono, 400B encoded, fits MAX_PACKET_SIZE
const FRAME_SAMPLES = 800;

const STEP = [
  7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45,
  50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230,
  253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963,
  1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327,
  3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442,
  11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794,
  32767,
];
const INDEX_ADJ = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

export function adpcmEncode(pcm: Int16Array): Uint8Array {
  let value = 0;
  let idx = 0;
  const out = new Uint8Array(Math.ceil(pcm.length / 2));
  for (let i = 0; i < pcm.length; i++) {
    const step = STEP[idx];
    let diff = pcm[i] - value;
    let code = 0;
    if (diff < 0) {
      code = 8;
      diff = -diff;
    }
    if (diff >= step) {
      code |= 4;
      diff -= step;
    }
    if (diff >= step >> 1) {
      code |= 2;
      diff -= step >> 1;
    }
    if (diff >= step >> 2) code |= 1;
    let delta = step >> 3;
    if (code & 4) delta += step;
    if (code & 2) delta += step >> 1;
    if (code & 1) delta += step >> 2;
    value = Math.max(-32768, Math.min(32767, value + (code & 8 ? -delta : delta)));
    idx = Math.max(0, Math.min(88, idx + INDEX_ADJ[code]));
    if (i & 1) out[i >> 1] |= code;
    else out[i >> 1] = code << 4;
  }
  return out;
}

export function adpcmDecode(buf: Uint8Array): Int16Array {
  const out = new Int16Array(buf.length * 2);
  let value = 0;
  let idx = 0;
  for (let i = 0; i < buf.length * 2; i++) {
    const code = i & 1 ? buf[i >> 1] & 0x0f : buf[i >> 1] >> 4;
    const step = STEP[idx];
    let delta = step >> 3;
    if (code & 4) delta += step;
    if (code & 2) delta += step >> 1;
    if (code & 1) delta += step >> 2;
    value = Math.max(-32768, Math.min(32767, value + (code & 8 ? -delta : delta)));
    idx = Math.max(0, Math.min(88, idx + INDEX_ADJ[code]));
    out[i] = value;
  }
  return out;
}

// ---------------------------------------------------------------------------
// Transmit: getUserMedia → decimate to 8kHz → ADPCM frames
// ---------------------------------------------------------------------------

export class VoiceTx {
  private stream?: MediaStream;
  private ctx?: AudioContext;
  private proc?: ScriptProcessorNode;
  private src?: MediaStreamAudioSourceNode;
  private acc: number[] = [];

  /** Start capturing. onFrame fires per ~100ms chunk; `last` on release. */
  async start(onFrame: (data: Uint8Array, last: boolean) => void): Promise<void> {
    this.stream = await navigator.mediaDevices.getUserMedia({
      audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true },
    });
    this.ctx = new AudioContext();
    this.src = this.ctx.createMediaStreamSource(this.stream);
    this.proc = this.ctx.createScriptProcessor(2048, 1, 1);
    const ratio = this.ctx.sampleRate / VOICE_RATE;
    let pend = 0;
    let sum = 0;
    this.proc.onaudioprocess = (ev) => {
      const buf = ev.inputBuffer.getChannelData(0);
      for (let i = 0; i < buf.length; i++) {
        sum += buf[i];
        if (++pend >= ratio) {
          // box-filter decimation — fine for speech bandwidth
          this.acc.push(Math.max(-32768, Math.min(32767, Math.round((sum / pend) * 32768))));
          pend = 0;
          sum = 0;
        }
      }
      while (this.acc.length >= FRAME_SAMPLES) {
        const chunk = Int16Array.from(this.acc.splice(0, FRAME_SAMPLES));
        onFrame(adpcmEncode(chunk), false);
      }
    };
    this.src.connect(this.proc);
    this.proc.connect(this.ctx.destination);
  }

  /** Stop capturing; emits a final partial frame marked `last`. */
  stop(onFrame: (data: Uint8Array, last: boolean) => void): void {
    this.proc?.disconnect();
    this.src?.disconnect();
    this.stream?.getTracks().forEach((t) => t.stop());
    void this.ctx?.close();
    this.proc = undefined;
    this.src = undefined;
    this.ctx = undefined;
    this.stream = undefined;
    if (this.acc.length > 0) {
      onFrame(adpcmEncode(Int16Array.from(this.acc.splice(0))), true);
    } else {
      onFrame(new Uint8Array(0), true); // pure end-of-transmission marker
    }
  }
}

// ---------------------------------------------------------------------------
// Receive: per-transmission playheads, ~120ms jitter buffer
// ---------------------------------------------------------------------------

export class VoiceRx {
  private ctx?: AudioContext;
  private playheads = new Map<string, number>();

  push(
    fromKey: string,
    codec: number,
    txId: number,
    _seq: number,
    last: boolean,
    data: Uint8Array,
  ): void {
    if (codec !== VOICE_CODEC_ADPCM || data.length === 0) {
      if (last || data.length === 0) this.playheads.delete(`${fromKey}|${txId}`);
      return;
    }
    const ctx = (this.ctx ??= new AudioContext());
    if (ctx.state === "suspended") void ctx.resume();
    const pcm = adpcmDecode(data);
    const f32 = new Float32Array(pcm.length);
    for (let i = 0; i < pcm.length; i++) f32[i] = pcm[i] / 32768;
    const buf = ctx.createBuffer(1, f32.length, VOICE_RATE);
    buf.copyToChannel(f32, 0);
    const key = `${fromKey}|${txId}`;
    const prev = this.playheads.get(key) ?? 0;
    // if the head went stale (>400ms gap), restart the buffer rather than
    // scheduling far in the past (those frames are dropped)
    const t = Math.max(ctx.currentTime + 0.12, prev < ctx.currentTime - 0.4 ? ctx.currentTime + 0.12 : prev);
    const src = ctx.createBufferSource();
    src.buffer = buf;
    src.connect(ctx.destination);
    src.start(t);
    this.playheads.set(key, t + buf.duration);
  }
}
