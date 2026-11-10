export interface Env {
  RELAY: DurableObjectNamespace;
  CLIENT_TOKEN: string;
  DEVICE_TOKEN: string;
  ALLOW_QUERY_KEY?: string;
  ALLOWED_ORIGINS?: string;
}
export const MAX_BYTES = 256 * 1024;
export const MAX_PENDING = 32;
export const MAX_CONTROL_PENDING = 8;
export const WAIT_MS = 240_000;
const encoder = new TextEncoder();
type JsonObject = Record<string, unknown>;
function object(value: unknown): value is JsonObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
export function reply(status: number, value?: unknown, extra?: HeadersInit): Response {
  const headers = new Headers(extra);
  headers.set('Cache-Control', 'no-store');
  headers.set('Content-Type', 'application/json');
  return new Response(value === undefined ? null : JSON.stringify(value), { status, headers });
}
// Compare fixed-length SHA-256 digests with the runtime's constant-time primitive.
export async function tokenEqual(candidate: string, expected: string): Promise<boolean> {
  const [a, b] = await Promise.all([candidate, expected].map(s =>
    crypto.subtle.digest('SHA-256', encoder.encode(s))));
  return crypto.subtle.timingSafeEqual(a, b);
}
export async function authorize(request: Request, env: Env): Promise<Response | null> {
  const url = new URL(request.url);
  if (!env.CLIENT_TOKEN || !env.DEVICE_TOKEN || await tokenEqual(env.CLIENT_TOKEN, env.DEVICE_TOKEN)) {
    return reply(503, { error: 'Relay authentication unavailable' });
  }
  const origin = request.headers.get('Origin');
  if (url.pathname === '/device' && origin !== null) return reply(403, { error: 'Browser device connections are not allowed' });
  const allowed = env.ALLOWED_ORIGINS === undefined
    ? ['https://chatgpt.com', url.origin]
    : env.ALLOWED_ORIGINS.split(',').map(s => s.trim()).filter(Boolean);
  if (origin !== null && !allowed.includes(origin)) return reply(403, { error: 'Origin denied' });
  const bearer = request.headers.get('Authorization');
  const pathKey = url.pathname.match(/^\/mcp\/([a-zA-Z0-9_-]{32,128})$/)?.[1];
  const candidate = bearer?.startsWith('Bearer ') ? bearer.slice(7)
    : !bearer && pathKey ? pathKey
    : url.pathname === '/mcp' && env.ALLOW_QUERY_KEY === 'true' && !bearer ? url.searchParams.get('key') : null;
  const expected = url.pathname === '/device' ? env.DEVICE_TOKEN : env.CLIENT_TOKEN;
  if (candidate === null || candidate.length > 4096 || !await tokenEqual(candidate, expected)) {
    return reply(401, { error: 'Unauthorized' }, { 'WWW-Authenticate': 'Bearer' });
  }
  return null;
}
export function acceptsJson(header: string | null): boolean {
  return !!header && header.split(',').some(part => {
    const [media, ...parameters] = part.trim().toLowerCase().split(';');
    const quality = parameters.find(p => p.trim().startsWith('q='));
    const q = quality ? Number(quality.trim().slice(2)) : 1;
    return ['application/json', 'application/*', '*/*'].includes(media.trim()) && q > 0 && q <= 1;
  });
}
async function readJson(request: Request): Promise<unknown> {
  if (Number(request.headers.get('Content-Length')) > MAX_BYTES) throw new Error('large');
  const reader = request.body?.getReader();
  if (!reader) throw new Error('json');
  const chunks: Uint8Array[] = [];
  let length = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    length += value.byteLength;
    if (length > MAX_BYTES) { await reader.cancel(); throw new Error('large'); }
    chunks.push(value);
  }
  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
  return JSON.parse(new TextDecoder('utf-8', { fatal: true, ignoreBOM: false }).decode(bytes));
}
export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const publicPath = new URL(request.url).pathname;
    if (publicPath === '/health' && request.method === 'GET') return reply(200, { service: 'slingring-relay', ok: true });
    const denied = await authorize(request, env);
    if (denied) return denied;
    const path = new URL(request.url).pathname;
    if (!['/mcp', '/device'].includes(path) && !/^\/mcp\/[a-zA-Z0-9_-]{32,128}$/.test(path)) return reply(404, { error: 'Not found' });
    // Authentication is repeated inside the DO; never forward a query token.
    const headers = new Headers(request.headers);
    headers.set('Authorization', `Bearer ${path === '/device' ? env.DEVICE_TOKEN : env.CLIENT_TOKEN}`);
    const url = new URL(request.url);
    url.search = '';
    url.pathname = path === '/device' ? '/device' : '/mcp';
    return env.RELAY.get(env.RELAY.idFromName('mac')).fetch(new Request(url, {
      method: request.method, headers, body: request.body, redirect: 'manual',
    }));
  },
};
type Pending = { finish: (response: Response) => void; notification: boolean; control: boolean; receipt: string };
// Reserve a bounded independent lane for recovery and MCP handshake requests.
// Tool arguments and all authorization decisions still belong to Rust.
function controlRequest(body: JsonObject): boolean {
  return ['initialize', 'ping', 'tools/list', 'notifications/initialized'].includes(String(body.method)) ||
    body.method === 'tools/call' && object(body.params) && body.params.name === 'get_job_status';
}
export class Relay {
  private pending = new Map<string, Pending>();
  private activeSocket: WebSocket | undefined;
  constructor(private ctx: DurableObjectState, private env: Env) {
    this.activeSocket = ctx.getWebSockets('device')[0];
    ctx.setWebSocketAutoResponse(new WebSocketRequestResponsePair('ping', 'pong'));
  }
  private socket(): WebSocket | undefined { return this.activeSocket; }
  async fetch(request: Request): Promise<Response> {
    const denied = await authorize(request, this.env);
    if (denied) return denied;
    const path = new URL(request.url).pathname;
    if (path === '/health') return request.method === 'GET'
      ? reply(200, { online: !!this.socket() }) : reply(405, { error: 'Method not allowed' }, { Allow: 'GET' });
    if (path === '/device') {
      if (request.method !== 'GET') return reply(405, { error: 'Method not allowed' }, { Allow: 'GET' });
      if (request.headers.get('Upgrade')?.toLowerCase() !== 'websocket') return reply(426, { error: 'WebSocket required' });
      const previous = this.socket();
      if (previous) {
        this.lost(previous);
        previous.close(1000, 'Replaced by an authenticated device connection');
      }
      const pair = new WebSocketPair();
      this.ctx.acceptWebSocket(pair[1], ['device']);
      this.activeSocket = pair[1];
      return new Response(null, { status: 101, webSocket: pair[0], headers: { 'Cache-Control': 'no-store' } });
    }
    if (path !== '/mcp') return reply(404, { error: 'Not found' });
    if (request.method !== 'POST') return reply(405, { error: 'Method not allowed' }, { Allow: 'POST' });
    const protocol = request.headers.get('MCP-Protocol-Version');
    if (protocol !== null && !['2025-03-26','2025-06-18','2025-11-25'].includes(protocol)) {
      return reply(400, { error: 'Unsupported MCP protocol version' });
    }
    if (request.headers.get('Content-Type')?.split(';')[0].trim().toLowerCase() !== 'application/json') {
      return reply(415, { error: 'application/json required' });
    }
    if (!acceptsJson(request.headers.get('Accept'))) return reply(406, { error: 'Accept application/json required' });
    const key = request.headers.get('Idempotency-Key');
    if (key !== null && !/^[a-zA-Z0-9_:-]{1,128}$/.test(key)) return reply(400, { error: 'Invalid Idempotency-Key' });
    let body: unknown;
    try { body = await readJson(request); }
    catch (error) { return reply(error instanceof Error && error.message === 'large' ? 413 : 400, { error: 'Invalid or oversized JSON body' }); }
    // Only framing is inspected. MCP methods, arguments, images and errors belong to Rust.
    if (!object(body)) return reply(400, { error: 'JSON object required' });
    const socket = this.socket();
    if (!socket) return reply(503, { error: 'Device offline' });
    const control = controlRequest(body);
    const pending = [...this.pending.values()].filter(item => item.control === control).length;
    if (pending >= (control ? MAX_CONTROL_PENDING : MAX_PENDING)) return reply(429, {
      error: control ? 'Relay control capacity exceeded; try this read-only lookup later' : 'Relay submission capacity exceeded; this HTTP attempt was not forwarded. Prior outcome for the same request ID may exist; use get_job_status',
    });
    const id = crypto.randomUUID();
    const notification = !Object.hasOwn(body, 'id');
    // The caller must know the ID BEFORE a slow request begins to recover a
    // result after its own timeout. Prefer its explicit request_id or header.
    const args = object(body.params) && object(body.params.arguments) ? body.params.arguments : null;
    const declared = typeof args?.request_id === 'string' && /^[A-Za-z0-9_:-]{1,128}$/.test(args.request_id)
      ? args.request_id : null;
    const receipt = key ?? declared ?? id;
    const frame = JSON.stringify({ type: 'rpc', id, request: body, expires_at: Date.now() + WAIT_MS - 10_000,
      ...(key === null ? {} : { idempotency_key: key }) });
    if (encoder.encode(frame).length > MAX_BYTES) return reply(413, { error: 'Relay frame too large' });
    return new Promise<Response>(resolve => {
      const timer = setTimeout(() => finish(reply(504, { error: 'Device response timeout. The Mac may still complete this job. Do not resubmit.',
        request_id: receipt, outcome: 'unknown', recovery_tool: 'get_job_status' })), WAIT_MS);
      const finish = (response: Response) => {
        clearTimeout(timer);
        this.pending.delete(id);
        // Useful when an HTTP response arrives, but NOT a replacement for
        // passing request_id before launch if the caller times out first.
        response.headers.set('X-Slingring-Request-Id', receipt);
        resolve(response);
      };
      this.pending.set(id, { finish, notification, control, receipt });
      try { socket.send(frame); }
      catch { this.lost(socket); }
    });
  }
  webSocketMessage(socket: WebSocket, message: string | ArrayBuffer): void {
    if (socket !== this.socket()) { socket.close(1008, 'Unknown device'); return; }
    // The hibernation runtime normally handles this without executing JS.
    // Retain a safe fallback for development runtimes and active-object delivery.
    if (message === 'ping') { socket.send('pong'); return; }
    if (typeof message !== 'string' || encoder.encode(message).length > MAX_BYTES) {
      this.lost(socket); socket.close(1009, 'Invalid frame'); return;
    }
    let frame: unknown;
    try { frame = JSON.parse(message); } catch { this.lost(socket); socket.close(1008, 'Invalid JSON'); return; }
    if (!object(frame) || frame.type !== 'rpc_result' || typeof frame.id !== 'string' ||
        !Object.hasOwn(frame, 'response') || !(frame.response === null || object(frame.response))) {
      this.lost(socket); socket.close(1008, 'Invalid result'); return;
    }
    const pending = this.pending.get(frame.id);
    if (!pending) return; // Late/unknown IDs cannot settle any other caller.
    if (pending.notification && frame.response === null) pending.finish(reply(202));
    else if (!pending.notification && object(frame.response)) pending.finish(reply(200, frame.response));
    else { this.lost(socket); socket.close(1008, 'Mismatched result'); }
  }
  private lost(socket: WebSocket): void {
    if (socket !== this.activeSocket) return;
    this.activeSocket = undefined;
    for (const item of this.pending.values()) item.finish(reply(503, {
      error: 'Device disconnected; outcome unknown; do not automatically retry',
      request_id: item.receipt, outcome: 'unknown', recovery_tool: 'get_job_status',
    }));
  }
  webSocketClose(socket: WebSocket): void { this.lost(socket); socket.close(1000, 'Disconnected'); }
  webSocketError(socket: WebSocket): void { this.lost(socket); socket.close(1011, 'Device error'); }
}
