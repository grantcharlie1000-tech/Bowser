// Media support check for the settings page (Media section): pure logic with injectable probes. Nothing here
// touches the network or the DOM; `browserProbes()` is the only place that reads browser globals.
// Results hold fixed ids, labels and booleans only, never page-derived text.

export type CodecId = 'h264' | 'vp9' | 'av1' | 'hevc';
export type Verdict = 'hardware' | 'software' | 'unsupported' | 'unknown';
export type DrmId = 'widevine' | 'playready';
export type DrmState = 'available' | 'unavailable' | 'unknown';
export type CanPlay = '' | 'maybe' | 'probably';

export interface CodecSpec { readonly id: CodecId; readonly label: string; readonly contentType: string }

export const CODECS: readonly CodecSpec[] = [
  { id: 'h264', label: 'H.264', contentType: 'video/mp4; codecs="avc1.640028"' },
  { id: 'vp9', label: 'VP9', contentType: 'video/webm; codecs="vp09.00.10.08"' },
  { id: 'av1', label: 'AV1', contentType: 'video/mp4; codecs="av01.0.05M.08"' },
  { id: 'hevc', label: 'HEVC / H.265', contentType: 'video/mp4; codecs="hvc1.1.6.L93.B0"' },
];

export interface DrmSpec { readonly id: DrmId; readonly label: string; readonly systems: readonly string[] }

export const DRM_SYSTEMS: readonly DrmSpec[] = [
  { id: 'widevine', label: 'Widevine', systems: ['com.widevine.alpha'] },
  { id: 'playready', label: 'PlayReady', systems: ['com.microsoft.playready', 'com.microsoft.playready.recommendation'] },
];

export const DEFAULT_TIMEOUT_MS = 3000;

export interface DecodingResult { readonly supported: boolean; readonly smooth: boolean; readonly powerEfficient: boolean }

export interface DecodingConfig {
  readonly type: 'file';
  readonly video: { readonly contentType: string; readonly width: number; readonly height: number; readonly bitrate: number; readonly framerate: number };
}

export interface KeySystemConfig {
  readonly initDataTypes: readonly string[];
  readonly videoCapabilities: readonly { readonly contentType: string }[];
  readonly audioCapabilities: readonly { readonly contentType: string }[];
  readonly persistentState: 'not-allowed';
  readonly distinctiveIdentifier: 'not-allowed';
  readonly sessionTypes: readonly string[];
}

/** Each probe is optional: a missing one means the API does not exist and yields `unknown`. */
export interface MediaProbes {
  canPlayType?: (contentType: string) => string;
  isTypeSupported?: (contentType: string) => boolean;
  decodingInfo?: (config: DecodingConfig) => Promise<unknown>;
  /** Resolves when the key system is available, rejects when it is not. No MediaKeys are created. */
  requestKeySystemAccess?: (system: string, configs: readonly KeySystemConfig[]) => Promise<unknown>;
}

export interface CodecResult {
  readonly id: CodecId;
  readonly label: string;
  readonly verdict: Verdict;
  /** null when MediaCapabilities gave no answer. */
  readonly smooth: boolean | null;
  readonly canPlay: CanPlay | null;
  readonly mediaSource: boolean | null;
}

export interface DrmResult { readonly id: DrmId; readonly label: string; readonly state: DrmState }

export interface MediaReport { readonly codecs: readonly CodecResult[]; readonly drm: readonly DrmResult[] }

export interface CheckOptions { readonly timeoutMs?: number }

type Settled<T> = { readonly ok: true; readonly value: T } | { readonly ok: false };

/** Runs `fn`, turning a throw, a rejection or a stall past `ms` into `{ ok: false }`. */
export async function settle<T>(fn: () => T | Promise<T>, ms: number): Promise<Settled<T>> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const stalled = new Promise<Settled<T>>((resolve) => { timer = setTimeout(() => resolve({ ok: false }), ms); });
  const run = (async (): Promise<Settled<T>> => {
    try { return { ok: true, value: await fn() }; } catch { return { ok: false }; }
  })();
  try { return await Promise.race([run, stalled]); } finally { clearTimeout(timer); }
}

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null;

function parseDecoding(value: unknown): DecodingResult | null {
  if (!isRecord(value) || typeof value.supported !== 'boolean') return null;
  return { supported: value.supported, smooth: value.smooth === true, powerEfficient: value.powerEfficient === true };
}

function parseCanPlay(value: unknown): CanPlay | null {
  return value === '' || value === 'maybe' || value === 'probably' ? value : null;
}

/** hardware = supported and power efficient, software = supported otherwise. */
export function classify(decoding: DecodingResult | null, canPlay: CanPlay | null, mediaSource: boolean | null): Verdict {
  if (decoding) {
    if (!decoding.supported) return 'unsupported';
    return decoding.powerEfficient ? 'hardware' : 'software';
  }
  // Without MediaCapabilities hardware or software cannot be told apart; only a unanimous "no" is certain.
  const answers = [canPlay === null ? null : canPlay !== '', mediaSource];
  const known = answers.filter((answer): answer is boolean => answer !== null);
  if (known.length > 0 && known.every((answer) => !answer)) return 'unsupported';
  return 'unknown';
}

export function decodingConfig(spec: CodecSpec): DecodingConfig {
  return { type: 'file', video: { contentType: spec.contentType, width: 1920, height: 1080, bitrate: 5000000, framerate: 30 } };
}

export function keySystemConfig(): KeySystemConfig {
  return {
    initDataTypes: ['cenc'],
    videoCapabilities: [{ contentType: 'video/mp4; codecs="avc1.640028"' }],
    audioCapabilities: [{ contentType: 'audio/mp4; codecs="mp4a.40.2"' }],
    persistentState: 'not-allowed',
    distinctiveIdentifier: 'not-allowed',
    sessionTypes: ['temporary'],
  };
}

async function checkCodec(spec: CodecSpec, probes: MediaProbes, ms: number): Promise<CodecResult> {
  const { canPlayType, isTypeSupported, decodingInfo } = probes;
  const [play, source, info] = await Promise.all([
    canPlayType ? settle(() => canPlayType(spec.contentType), ms) : null,
    isTypeSupported ? settle(() => isTypeSupported(spec.contentType), ms) : null,
    decodingInfo ? settle(() => decodingInfo(decodingConfig(spec)), ms) : null,
  ]);
  const canPlay = play?.ok ? parseCanPlay(play.value) : null;
  const mediaSource = source?.ok && typeof source.value === 'boolean' ? source.value : null;
  const decoding = info?.ok ? parseDecoding(info.value) : null;
  return {
    id: spec.id,
    label: spec.label,
    verdict: classify(decoding, canPlay, mediaSource),
    smooth: decoding ? decoding.smooth : null,
    canPlay,
    mediaSource,
  };
}

async function checkDrm(spec: DrmSpec, probes: MediaProbes, ms: number): Promise<DrmResult> {
  const request = probes.requestKeySystemAccess;
  if (!request) return { id: spec.id, label: spec.label, state: 'unknown' };
  for (const system of spec.systems) {
    const outcome = await settle(() => request(system, [keySystemConfig()]), ms);
    if (outcome.ok) return { id: spec.id, label: spec.label, state: 'available' };
  }
  return { id: spec.id, label: spec.label, state: 'unavailable' };
}

/** Runs every probe once. Never rejects: a failing or stalled probe only lowers that row to unknown or unavailable. */
export async function runMediaCheck(probes: MediaProbes, options: CheckOptions = {}): Promise<MediaReport> {
  const ms = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const [codecs, drm] = await Promise.all([
    Promise.all(CODECS.map((spec) => checkCodec(spec, probes, ms))),
    Promise.all(DRM_SYSTEMS.map((spec) => checkDrm(spec, probes, ms))),
  ]);
  return { codecs, drm };
}

/** Probes backed by the real browser APIs. Detached elements only: nothing is attached or played. */
export function browserProbes(): MediaProbes {
  const probes: MediaProbes = {};
  if (typeof document !== 'undefined') {
    const video = document.createElement('video');
    probes.canPlayType = (type) => video.canPlayType(type);
  }
  if (typeof MediaSource !== 'undefined' && typeof MediaSource.isTypeSupported === 'function') {
    probes.isTypeSupported = (type) => MediaSource.isTypeSupported(type);
  }
  if (typeof navigator !== 'undefined') {
    const capabilities = navigator.mediaCapabilities;
    if (capabilities && typeof capabilities.decodingInfo === 'function') {
      probes.decodingInfo = (config) => capabilities.decodingInfo(config as unknown as MediaDecodingConfiguration);
    }
    if (typeof navigator.requestMediaKeySystemAccess === 'function') {
      probes.requestKeySystemAccess = (system, configs) => navigator.requestMediaKeySystemAccess(system, configs as unknown as MediaKeySystemConfiguration[]);
    }
  }
  return probes;
}

export function verdictText(verdict: Verdict): string {
  switch (verdict) {
    case 'hardware': return 'Hardware decode';
    case 'software': return 'Software decode';
    case 'unsupported': return 'Not supported';
    default: return 'Unknown';
  }
}

export function drmText(state: DrmState): string {
  switch (state) {
    case 'available': return 'Available';
    case 'unavailable': return 'Not available';
    default: return 'Unknown';
  }
}

const yesNo = (value: boolean): string => (value ? 'yes' : 'no');

/** One short line of the raw answers behind a codec row. */
export function codecDetail(result: CodecResult): string {
  const parts: string[] = [];
  if (result.canPlay !== null) parts.push(`Player: ${result.canPlay === '' ? 'no' : result.canPlay}`);
  if (result.mediaSource !== null) parts.push(`Streaming: ${yesNo(result.mediaSource)}`);
  if (result.smooth !== null) parts.push(`Smooth at 1080p: ${yesNo(result.smooth)}`);
  return parts.length > 0 ? `${parts.join('. ')}.` : 'This browser did not report support.';
}

/** Plain-text advice for the report, in a stable order. Empty when nothing needs attention. */
export function guidance(report: MediaReport): string[] {
  const lines: string[] = [];
  for (const codec of report.codecs) {
    if (codec.verdict === 'unsupported') {
      if (codec.id === 'hevc') lines.push('HEVC is not supported. Install the HEVC Video Extension from the Microsoft Store (Windows) so video can use hardware decode.');
      else if (codec.id === 'av1') lines.push('AV1 is not supported. Install the AV1 Video Extension from the Microsoft Store (Windows) so video can use hardware decode.');
      else lines.push(`${codec.label} is not supported. Update your graphics driver and Windows so ${codec.label} video can play.`);
    } else if (codec.verdict === 'software') {
      lines.push(`${codec.label} decodes in software. The GPU driver may not offer hardware decode for it, so battery use can be higher. Updating the graphics driver can help.`);
    }
    if (codec.smooth === false && (codec.verdict === 'hardware' || codec.verdict === 'software')) {
      lines.push(`${codec.label} at 1080p may not play smoothly on this device.`);
    }
  }
  for (const system of report.drm) {
    if (system.state === 'unavailable') {
      lines.push(`${system.label} is not available. Streaming services that need ${system.label} may not play.`);
    }
  }
  return lines;
}
