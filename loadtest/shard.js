'use strict';

// Orquestador de UN shard de carga para CI.
//
// Reparte los bots entre varios workers (uno por CPU del runner), junta las
// estadísticas y corta todo al terminar la ventana warmup+medición.
// No usa RCON: la métrica del server (TPS/MSPT/CPU) se recolecta del lado del
// VPS; este proceso solo genera carga y reporta lo que ven los bots.
//
// Uso:
//   node shard.js --host 1.2.3.4 --port 25565 --count 25 --workers 4 \
//                 --warmup 45 --duration 90 --prefix Perf --shard 0 --startIndex 1

const { fork } = require('child_process');
const path = require('path');

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    if (!argv[i].startsWith('--')) continue;
    const key = argv[i].slice(2);
    const val = argv[i + 1] && !argv[i + 1].startsWith('--') ? argv[i + 1] : 'true';
    out[key] = val;
  }
  return out;
}

const a = parseArgs(process.argv.slice(2));
const cfg = {
  host: a.host || '127.0.0.1',
  port: parseInt(a.port || '25565', 10),
  count: parseInt(a.count || '25', 10),
  workers: parseInt(a.workers || '4', 10),
  duration: parseInt(a.duration || '90', 10),
  warmup: parseInt(a.warmup || '45', 10),
  prefix: a.prefix || 'Perf',
  version: a.version || '1.8.9',
  shard: parseInt(a.shard || '0', 10),
  startIndex: parseInt(
    a.startIndex || String(parseInt(a.shard || '0', 10) * parseInt(a.count || '25', 10) + 1),
    10
  ),
  arenaX: parseInt(a.arenaX || '100', 10),
  arenaZ: parseInt(a.arenaZ || '100', 10),
};

const startedAt = Date.now();
const workers = [];
const lastStats = [];

const log = (...x) => console.log(`[shard ${cfg.shard}]`, ...x);

function spawnWorker(i) {
  const per = Math.floor(cfg.count / cfg.workers);
  const remainder = cfg.count % cfg.workers;
  const myCount = per + (i < remainder ? 1 : 0);
  let offset = 0;
  for (let j = 0; j < i; j++) offset += per + (j < remainder ? 1 : 0);

  const child = fork(
    path.join(__dirname, 'worker.js'),
    [
      JSON.stringify({
        worker: `${cfg.shard}.${i}`,
        host: cfg.host,
        port: cfg.port,
        count: myCount,
        start: cfg.startIndex + offset,
        version: cfg.version,
        prefix: cfg.prefix,
        arena: { x: cfg.arenaX, z: cfg.arenaZ },
      }),
    ],
    { stdio: ['ignore', 'inherit', 'inherit', 'ipc'] }
  );
  child.on('message', (m) => {
    if (m && m.type === 'stats') lastStats[i] = m;
  });
  child.on('exit', (code) => log(`worker ${i} salio (code=${code})`));
  workers.push(child);
}

for (let i = 0; i < cfg.workers; i++) spawnWorker(i);

const sum = (k) => lastStats.filter(Boolean).reduce((x, y) => x + (y[k] || 0), 0);

const tick = setInterval(() => {
  const elapsed = Math.round((Date.now() - startedAt) / 1000);
  const phase = elapsed < cfg.warmup ? 'warmup' : 'medicion';
  log(
    `t=${elapsed}s fase=${phase} vivos=${sum('alive')}/${cfg.count} ` +
      `muertes=${sum('deaths')} ataques=${sum('attacks')} err=${sum('errors')} ` +
      `kicks=${sum('kicks')} sinruta=${sum('stuck')} arena=${sum('reached')}`
  );
}, 10000);

const totalTest = cfg.warmup + cfg.duration;

function stopAll() {
  clearInterval(tick);
  for (const c of workers) {
    try {
      c.send({ type: 'stop' });
    } catch (_) {}
  }
  setTimeout(() => {
    const summary = {
      shard: cfg.shard,
      host: cfg.host,
      port: cfg.port,
      total: cfg.count,
      workers: cfg.workers,
      warmup: cfg.warmup,
      duration: cfg.duration,
      prefix: cfg.prefix,
      alive: sum('alive'),
      deaths: sum('deaths'),
      attacks: sum('attacks'),
      errors: sum('errors'),
      kicks: sum('kicks'),
      reached: sum('reached'),
      stuck: sum('stuck'),
    };
    console.log('SHARD_SUMMARY ' + JSON.stringify(summary));
    process.exit(0);
  }, 4000);
}

setTimeout(stopAll, (totalTest + 5) * 1000);
process.on('SIGTERM', stopAll);
process.on('SIGINT', stopAll);
