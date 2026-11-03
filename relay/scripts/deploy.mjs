import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { readFile, rm, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const relayDirectory = resolve(dirname(fileURLToPath(import.meta.url)), '..');

/** Add only nonsecret deployment identifiers to the public Wrangler template. */
export function productionConfig(base, environment) {
  const required = ['CLOUDFLARE_ACCOUNT_ID', 'RELAY_ZONE_ID', 'RELAY_HOSTNAME'];
  for (const key of required) {
    if (!environment[key]) {
      throw new Error(`Missing ${key}; configure the existing deployment identifiers before deploying`);
    }
  }
  if (!/^[a-f\d]{32}$/i.test(environment.CLOUDFLARE_ACCOUNT_ID)) {
    throw new Error('CLOUDFLARE_ACCOUNT_ID must be a 32-character hex identifier');
  }
  if (!/^[a-f\d]{32}$/i.test(environment.RELAY_ZONE_ID)) {
    throw new Error('RELAY_ZONE_ID must be a 32-character hex identifier');
  }
  const hostname = environment.RELAY_HOSTNAME;
  const dnsLabel = '[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?';
  if (hostname.length > 253 || !new RegExp(`^(?:${dnsLabel}\\.)+${dnsLabel}$`, 'i').test(hostname)) {
    throw new Error('RELAY_HOSTNAME must be a DNS hostname without a scheme, path, port or wildcard');
  }
  if (!base.name || 'account_id' in base || 'route' in base || 'routes' in base) {
    throw new Error('Public Wrangler template must define a Worker name without account or route bindings');
  }
  return {
    ...base,
    account_id: environment.CLOUDFLARE_ACCOUNT_ID,
    routes: [{ pattern: hostname, custom_domain: true, zone_id: environment.RELAY_ZONE_ID }],
  };
}

async function main(args) {
  if (args.length > 1 || (args.length === 1 && args[0] !== '--dry-run')) {
    throw new Error('Usage: bun run deploy:configured [--dry-run]');
  }
  const base = JSON.parse(await readFile(join(relayDirectory, 'wrangler.jsonc'), 'utf8'));
  const config = productionConfig(base, process.env);
  // The config lives beside the public template so relative main/binding paths
  // resolve identically. Create exclusively, then remove it even after failure.
  const filename = `wrangler.deploy.${randomUUID()}.jsonc`;
  const temporary = join(relayDirectory, filename);
  await writeFile(temporary, `${JSON.stringify(config, null, 2)}\n`, { flag: 'wx', mode: 0o600 });
  try {
    const cli = process.platform === 'win32' ? 'bunx.cmd' : 'bunx';
    // Keep any nonsecret variables the operator configured in Cloudflare's
    // dashboard; Wrangler also preserves existing Worker secrets on deploy.
    const flags = ['--keep-vars', ...(args.length ? ['--dry-run', '--outdir', 'dist'] : [])];
    const exitCode = await new Promise((resolveExit, reject) => {
      const child = spawn(cli, ['wrangler', 'deploy', '--config', filename, ...flags], {
        cwd: relayDirectory,
        stdio: 'inherit',
      });
      child.once('error', reject);
      child.once('exit', code => resolveExit(code ?? 1));
    });
    process.exitCode = exitCode;
  } finally {
    await rm(temporary);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch(error => {
    console.error(`Relay deployment preparation failed: ${error.message}`);
    process.exitCode = 1;
  });
}
