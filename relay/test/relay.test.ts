import { webcrypto } from 'node:crypto';
import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
import worker, { Relay, authorize, acceptsJson, MAX_BYTES, WAIT_MS } from '../src/index';
// Node lacks the Workers timing-safe digest primitive.
import { timingSafeEqual } from 'node:crypto';
Object.defineProperty(globalThis, 'crypto', { value: webcrypto, configurable: true });
Object.assign(crypto.subtle, { timingSafeEqual: (a: ArrayBuffer, b: ArrayBuffer) => timingSafeEqual(Buffer.from(a), Buffer.from(b)) });
class Socket {
  frames: string[] = [];
  closed = false;
  send(frame: string) { if (this.closed) throw Error('closed'); this.frames.push(frame); }
  close() { this.closed = true; }
}
let socket: Socket;
let relay: Relay;
let env: any;
let context: any;
function req(body: unknown = { jsonrpc: '2.0', id: 1, method: 'tools/call', params: { images: ['unchanged'] } }, headers = {}, path = '/mcp', method = 'POST') {
  return new Request(`https://relay.example${path}`, { method, headers: { Authorization: 'Bearer client', Accept: 'application/json', 'Content-Type': 'application/json', ...headers }, ...(method === 'GET' ? {} : { body: JSON.stringify(body) }) });
}
async function sent() { for (let i = 0; i < 30 && socket.frames.length === 0; i++) await new Promise(resolve => setImmediate(resolve)); }
function result(response: unknown, id = JSON.parse(socket.frames.at(-1)!).id) { relay.webSocketMessage(socket as any, JSON.stringify({ type: 'rpc_result', id, response })); }
beforeEach(() => {
  socket = new Socket();
  env = { CLIENT_TOKEN: 'client', DEVICE_TOKEN: 'device' };
  context = { getWebSockets: () => socket.closed ? [] : [socket], setWebSocketAutoResponse: vi.fn(), acceptWebSocket: vi.fn() };
  vi.stubGlobal('WebSocketRequestResponsePair', class {});
  relay = new Relay(context, env);
});
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });
describe('authentication and origin', () => {
  it('requires route-specific tokens and rejects equal/missing secrets', async () => {
    expect((await authorize(req({}, {}, '/device'), env))?.status).toBe(401);
    expect(await authorize(req({}, { Authorization: 'Bearer device' }, '/device'), env)).toBeNull();
    expect((await authorize(req({}, { Authorization: 'Bearer wrong' }), env))?.status).toBe(401);
    expect((await authorize(req(), { ...env, DEVICE_TOKEN: 'client' }))?.status).toBe(503);
    expect((await authorize(req(), { ...env, CLIENT_TOKEN: '' }))?.status).toBe(503);
  });
  it('query auth is opt in, mcp only, and does not override a bad header', async () => {
    const query = new Request('https://relay.example/mcp?key=client');
    expect((await authorize(query, env))?.status).toBe(401);
    expect(await authorize(query, { ...env, ALLOW_QUERY_KEY: 'true' })).toBeNull();
    expect((await authorize(new Request('https://relay.example/device?key=device'), { ...env, ALLOW_QUERY_KEY: 'true' }))?.status).toBe(401);
  });
  it('checks exact origins and permits absent Origin', async () => {
    for (const Origin of ['https://chatgpt.com', 'https://relay.example']) expect(await authorize(req({}, { Origin }), env)).toBeNull();
    for (const Origin of ['null', 'https://chatgpt.com.evil', 'http://chatgpt.com']) expect((await authorize(req({}, { Origin }), env))?.status).toBe(403);
    expect(await authorize(req({}, { Origin: 'https://custom.example' }), { ...env, ALLOWED_ORIGINS: 'https://custom.example' })).toBeNull();
    expect(await authorize(req(), env)).toBeNull();
  });
});
describe('HTTP framing', () => {
  it('enforces method, content type, Accept and bounded bodies', async () => {
    expect((await relay.fetch(req({}, {}, '/mcp', 'GET'))).status).toBe(405);
    expect((await relay.fetch(req({}, { 'Content-Type': 'text/plain' }))).status).toBe(415);
    expect((await relay.fetch(req({}, { Accept: 'text/event-stream' }))).status).toBe(406);
    expect((await relay.fetch(req('x'.repeat(MAX_BYTES)))).status).toBe(413);
    expect((await relay.fetch(req([]))).status).toBe(400);
    expect((await relay.fetch(new Request('https://relay.example/mcp', { method: 'POST', headers: { Authorization: 'Bearer client', Accept: 'application/json', 'Content-Type': 'application/json' }, body: '{' }))).status).toBe(400);
    expect(acceptsJson('application/json;q=0')).toBe(false);
    expect(acceptsJson('*/*')).toBe(true);
  });
  it('preserves arbitrary JSON and idempotency key with no policy filtering', async () => {
    const promise = relay.fetch(req(undefined, { 'Idempotency-Key': 'one' }));
    await sent();
    const frame = JSON.parse(socket.frames[0]);
    expect(frame.request.params.images).toEqual(['unchanged']);
    expect(frame.idempotency_key).toBe('one');
    const response = { jsonrpc: '2.0', id: 1, result: { arbitrary: ['data'] } };
    result(response);
    const reply = await promise;
    expect(await reply.json()).toEqual(response);
    expect(reply.headers.get('Cache-Control')).toBe('no-store');
    expect(reply.headers.get('Content-Type')).toBe('application/json');
  });
  it('acknowledges notifications only after daemon confirms delivery', async () => {
    const promise = relay.fetch(req({ jsonrpc: '2.0', method: 'notifications/initialized' }));
    await sent(); result(null);
    const response = await promise;
    expect(response.status).toBe(202); expect(await response.text()).toBe('');
  });
  it('fails offline and exposes only boolean health', async () => {
    relay.webSocketClose(socket as any);
    expect((await relay.fetch(req())).status).toBe(503);
    expect(await (await relay.fetch(req({}, {}, '/health', 'GET'))).json()).toEqual({ online: false });
  });
  it('requires a native authenticated WebSocket for device connections', async () => {
    expect((await relay.fetch(req({}, { Authorization: 'Bearer device' }, '/device', 'GET'))).status).toBe(426);
    expect((await relay.fetch(req({}, { Authorization: 'Bearer device', Upgrade: 'websocket', Origin: 'https://chatgpt.com' }, '/device', 'GET'))).status).toBe(403);
    expect(socket.closed).toBe(false);
  });
  it('accepts an unguessable MCP URL but never a device token in the URL', async () => {
    const token = 'a'.repeat(64);
    expect(await authorize(new Request(`https://relay.example/mcp/${token}`), {...env,CLIENT_TOKEN:token})).toBeNull();
    expect((await authorize(new Request(`https://relay.example/mcp/${token}`,{headers:{Authorization:'Bearer wrong'}}), {...env,CLIENT_TOKEN:token}))?.status).toBe(401);
    expect((await authorize(new Request(`https://relay.example/device/${token}`), {...env,DEVICE_TOKEN:token}))?.status).toBe(401);
  });
  it('rejects unsupported protocol versions and invalid idempotency keys', async () => {
    expect((await relay.fetch(req({}, {'MCP-Protocol-Version':'1900-01-01'}))).status).toBe(400);
    expect((await relay.fetch(req({}, {'Idempotency-Key':'../some/path'}))).status).toBe(400);
  });
  it('authenticates worker before forwarding and strips query secrets', async () => {
    const fetch = vi.fn(async (request: Request) => { expect(request.url).toBe('https://relay.example/mcp'); return new Response(); });
    env.RELAY = { idFromName: () => 'mac', get: () => ({ fetch }) };
    env.ALLOW_QUERY_KEY = 'true';
    await worker.fetch(new Request('https://relay.example/mcp?key=client'), env);
    expect(fetch).toHaveBeenCalledOnce();
    await worker.fetch(new Request('https://relay.example/mcp'), env);
    expect(fetch).toHaveBeenCalledOnce();
  });
});
describe('pending lifecycle', () => {
  it('caps concurrent requests at 32 and fails them on disconnect', async () => {
    const calls = Array.from({ length: 32 }, () => relay.fetch(req()));
    for (let i = 0; i < 50 && socket.frames.length < 32; i++) await new Promise(resolve => setImmediate(resolve));
    expect(socket.frames).toHaveLength(32);
    expect((await relay.fetch(req())).status).toBe(429);
    relay.webSocketClose(socket as any);
    expect((await Promise.all(calls)).every(r => r.status === 503)).toBe(true);
  });
  it('times out at 240 seconds without replay', async () => {
    const call = relay.fetch(req()); await sent(); vi.useFakeTimers({toFake:['setTimeout','clearTimeout']});
    // Timer was installed with real timers; use fake timers before starting another call.
    relay.webSocketError(socket as any); await call;
    socket = new Socket();
    relay = new Relay(context, env);
    const next = relay.fetch(req());
    await sent();
    await vi.advanceTimersByTimeAsync(WAIT_MS);
    expect((await next).status).toBe(504); expect(socket.frames).toHaveLength(1);
  });
  it.each(['garbage', JSON.stringify({ type: 'unknown' }), 'x'.repeat(MAX_BYTES + 1)])('rejects invalid frames', async message => {
    const call = relay.fetch(req()); await sent(); relay.webSocketMessage(socket as any, message);
    expect((await call).status).toBe(503); expect(socket.closed).toBe(true);
  });
});
