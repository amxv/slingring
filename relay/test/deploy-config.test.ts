import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { productionConfig } from '../scripts/deploy.mjs';

const base = JSON.parse(readFileSync(new URL('../wrangler.jsonc', import.meta.url), 'utf8'));
const identifiers = {
  CLOUDFLARE_ACCOUNT_ID: '1'.repeat(32),
  RELAY_ZONE_ID: '2'.repeat(32),
  RELAY_HOSTNAME: 'mcp.example.com',
};

describe('public Wrangler template and configured deployment', () => {
  it('does not bind another operator account or hostname by default', () => {
    expect(base.account_id).toBeUndefined();
    expect(base.route).toBeUndefined();
    expect(base.routes).toBeUndefined();
    expect(base.name).toBe('slingring-relay');
  });

  it('preserves Worker identity, migrations, Durable Object bindings and vars', () => {
    const configured = productionConfig(base, identifiers);
    expect(configured).toMatchObject({
      name: base.name,
      durable_objects: base.durable_objects,
      migrations: base.migrations,
      vars: base.vars,
      account_id: identifiers.CLOUDFLARE_ACCOUNT_ID,
      routes: [{ pattern: identifiers.RELAY_HOSTNAME, custom_domain: true, zone_id: identifiers.RELAY_ZONE_ID }],
    });
    expect(base).not.toHaveProperty('routes');
    expect(base).not.toHaveProperty('account_id');
  });

  it.each(['CLOUDFLARE_ACCOUNT_ID', 'RELAY_ZONE_ID', 'RELAY_HOSTNAME'])(
    'fails closed without %s', key => {
      expect(() => productionConfig(base, { ...identifiers, [key]: '' })).toThrow(`Missing ${key}`);
    },
  );

  it('rejects bad identifiers and malformed hostnames without exposing their values', () => {
    for (const [key, value] of [
      ['CLOUDFLARE_ACCOUNT_ID', 'not-an-id'],
      ['RELAY_ZONE_ID', 'not-an-id'],
      ['RELAY_HOSTNAME', 'https://mcp.example.com/path'],
      ['RELAY_HOSTNAME', '*.example.com'],
      ['RELAY_HOSTNAME', 'localhost:8787'],
    ]) {
      expect(() => productionConfig(base, { ...identifiers, [key]: value })).toThrow();
    }
  });

  it('rejects accidental source-template production bindings', () => {
    expect(() => productionConfig({ ...base, routes: [] }, identifiers)).toThrow('Public Wrangler template');
    expect(() => productionConfig({ ...base, account_id: '0'.repeat(32) }, identifiers)).toThrow('Public Wrangler template');
  });

  it('does not serialize deployment tokens even if the environment contains one', () => {
    const serialized = JSON.stringify(productionConfig(base, { ...identifiers, CLOUDFLARE_API_TOKEN: 'test-only-secret' }));
    expect(serialized).not.toContain('test-only-secret');
  });
});
