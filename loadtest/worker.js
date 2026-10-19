'use strict';

// Un worker = una CPU. Reparte los bots, los saca del spawn con pathfinder y
// los mete en un bucle de PvP contra los demas bots del prefijo.

const mineflayer = require('mineflayer');
const { pathfinder, Movements, goals } = require('mineflayer-pathfinder');
const { GoalXZ } = goals;

const cfg = JSON.parse(process.argv[2]);
const PHASES = Array.isArray(cfg.phases) && cfg.phases.length ? cfg.phases : null;
const bots = [];
const stats = { deaths: 0, attacks: 0, errors: 0, kicks: 0, reached: 0, stuck: 0, reconnects: 0, disconnects: 0 };
const startedAt = Date.now();
let shuttingDown = false;

// Fase actual segun el reloj compartido (cfg.t0). null = sin fases o test terminado.
function currentPhase() {
  if (!PHASES) return null;
  const elapsed = (Date.now() - (cfg.t0 || startedAt)) / 1000;
  let acc = 0;
  for (const p of PHASES) {
    acc += p.secs;
    if (elapsed < acc) return p;
  }
  return null;
}

const log = (...args) => console.log(`[w${cfg.worker}]`, ...args);

function createBot(index, attempt = 0) {
  const username = `${cfg.prefix}${String(index).padStart(2, '0')}`;
  const bot = mineflayer.createBot({
    host: cfg.host,
    port: cfg.port,
    username,
    version: cfg.version,
    auth: 'offline',
    checkTimeoutInterval: 60000,
    hideErrors: true,
  });
  bot.loadPlugin(pathfinder);

  const st = { mode: 'join', lastAttack: 0, lastJump: 0, lastGoal: 0, failures: 0, timer: null };
  bot._perf = st;

  const stopPath = () => {
    try { bot.pathfinder.stop(); } catch (_) {}
  };

  const setArenaGoal = (cx = cfg.arena.x, cz = cfg.arena.z, minR = 4, addR = 20) => {
    if (shuttingDown || !bot.entity) return;
    const angle = Math.random() * Math.PI * 2;
    const radius = minR + Math.random() * addR;
    const x = Math.round(cx + Math.cos(angle) * radius);
    const z = Math.round(cz + Math.sin(angle) * radius);
    try {
      const moves = new Movements(bot);
      moves.canDig = false;
      moves.allow1by1towers = false;
      moves.allowSprinting = true;
      moves.allowParkour = true;
      moves.scafoldingBlocks = [];
      bot.pathfinder.setMovements(moves);
      bot.pathfinder.setGoal(new GoalXZ(x, z));
      st.mode = 'travel';
      st.lastGoal = Date.now();
      bot.setControlState('sprint', true);
    } catch (e) {
      stats.errors++;
      st.mode = 'fallback';
      if (stats.errors < 10) log('pathfinder setup failed', username, e.message);
    }
  };

  const afterRespawn = () => {
    st.mode = 'join';
    st.lastJump = 0;
    st.lastGoal = 0;
    bot.clearControlStates();
    stopPath();
    setTimeout(() => setArenaGoal(), 1000);
  };

  bot.once('spawn', () => {
    log('spawn', username, bot.entity.position.toString());
    afterRespawn();
  });
  bot.on('respawn', () => {
    log('respawn', username);
    afterRespawn();
  });
  bot.on('death', () => {
    stats.deaths++;
    st.mode = 'dead';
    bot.clearControlStates();
    stopPath();
  });
  bot.on('goal_reached', () => {
    st.mode = 'arena';
    stats.reached++;
    bot.setControlState('sprint', false);
    bot.setControlState('forward', false);
  });
  bot.on('path_update', (r) => {
    if (r.status === 'noPath' || r.status === 'timeout') {
      stats.stuck++;
      if (st.mode === 'travel') {
        st.failures++;
        st.mode = st.failures > 1 ? 'fallback' : 'stuck';
      }
    }
  });
  bot.on('error', (e) => {
    stats.errors++;
    if (stats.errors < 15) log('error', username, e.message);
  });
  bot.on('kicked', (reason) => {
    stats.kicks++;
    log('kicked', username, JSON.stringify(reason).slice(0, 140));
  });
  bot.on('end', (reason) => {
    clearInterval(st.timer);
    const pos = bots.indexOf(bot);
    if (pos >= 0) bots.splice(pos, 1);
    if (shuttingDown) return;
    stats.disconnects++;
    const maxReconnects = cfg.maxReconnects || 10;
    if (attempt < maxReconnects) {
      stats.reconnects++;
      log('reconnect', username, `intento ${attempt + 1}/${maxReconnects}`, 'motivo', reason);
      setTimeout(() => { if (!shuttingDown) createBot(index, attempt + 1); }, 2500 + Math.random() * 2500);
    } else {
      log('end', username, reason);
    }
  });

  st.timer = setInterval(() => {
    if (shuttingDown || !bot.entity || bot.health === undefined) return;
    if (bot.health <= 0) return;

    const ph = currentPhase();
    if (PHASES && ph !== st.phase) {
      st.phase = ph;
      stopPath();
      bot.clearControlStates();
      st.lastGoal = 0;
      st.mode = ph && ph.mode === 'fight' ? 'arena' : 'travel';
    }
    if (ph && ph.mode === 'idle') return;
    const arena = ph && ph.x !== undefined ? { x: ph.x, z: ph.z } : cfg.arena;
    if (ph && ph.mode === 'move') {
      if (Date.now() - st.lastGoal > 8000) setArenaGoal(arena.x, arena.z, 0, 3);
      return;
    }

    const pos = bot.entity.position;
    const radius = Math.hypot(pos.x, pos.z);

    // Si el respawn no emitio evento, recuperamos por salud + posicion.
    if (st.mode === 'dead' && bot.health > 0) afterRespawn();

    // Dentro del spawn/safezone no se pelea: solo se camina hacia la arena.
    if (radius < 70) {
      if (st.mode === 'arena' || st.mode === 'fight') {
        st.mode = 'travel';
        setArenaGoal();
      }
      if (st.mode === 'join') return;
    }

    let best = null;
    let bestDist = Infinity;
    for (const e of Object.values(bot.entities)) {
      if (!e || e === bot.entity) continue;
      if (!e.username || !e.username.startsWith(cfg.prefix)) continue;
      if (e.isValid === false) continue;
      if (e.health !== undefined && e.health <= 0) continue;
      const d = e.position.distanceTo(pos);
      if (d < bestDist) {
        bestDist = d;
        best = e;
      }
    }

    if (radius >= 70 && best && bestDist < 24) {
      if (st.mode !== 'fight') {
        stopPath();
        st.mode = 'fight';
        bot.setControlState('sprint', false);
      }
      bot.lookAt(best.position.offset(0, 1.5, 0), true);
      if (bestDist > 3.0) {
        bot.setControlState('forward', true);
        if (Date.now() - st.lastJump > 900 && bot.entity.velocity.y === 0) {
          bot.setControlState('jump', true);
          st.lastJump = Date.now();
          setTimeout(() => { try { bot.setControlState('jump', false); } catch (_) {} }, 150);
        }
      } else {
        bot.setControlState('forward', false);
        if (Date.now() - st.lastAttack > 480) {
          try {
            bot.attack(best);
            stats.attacks++;
            st.lastAttack = Date.now();
          } catch (_) {}
        }
      }
      return;
    }

    if (st.mode === 'fight') {
      // El objetivo desaparecio o se alejo: volver a la arena.
      st.mode = 'arena';
      bot.setControlState('forward', false);
    }

    if (st.mode === 'stuck' || st.mode === 'arena') {
      if (Date.now() - st.lastGoal > 8000) setArenaGoal(arena.x, arena.z);
    } else if (st.mode === 'fallback') {
      // Ultimo recurso si el pathfinder no encuentra ruta: caminar recto.
      const dx = arena.x - pos.x;
      const dz = arena.z - pos.z;
      if (Math.hypot(dx, dz) < 4) {
        st.mode = 'arena';
        bot.clearControlStates();
        return;
      }
      bot.lookAt(pos.offset(dx, 0, dz), true);
      bot.setControlState('forward', true);
      bot.setControlState('sprint', true);
      if (Date.now() - st.lastJump > 1100) {
        bot.setControlState('jump', true);
        st.lastJump = Date.now();
        setTimeout(() => { try { bot.setControlState('jump', false); } catch (_) {} }, 150);
      }
    }
  }, 250);

  bots.push(bot);
}

// Conectar con una separacion pequena para no saturar ViaProxy de golpe.
const spacing = cfg.joinSpacing || 900;
for (let i = 0; i < cfg.count; i++) {
  setTimeout(() => createBot(cfg.start + i, 0), i * spacing);
}

setInterval(() => {
  const alive = bots.filter((b) => b.entity && b.player && (b.health ?? 0) > 0).length;
  const stuckNames = bots
    .filter((b) => b.entity && ['fallback', 'stuck'].includes(b._perf?.mode) && Math.hypot(b.entity.position.x, b.entity.position.z) < 65)
    .map((b) => b.player?.username)
    .filter(Boolean);
  const positions = bots
    .filter((b) => b.entity)
    .map((b) => [b.player?.username || '?', Math.round(b.entity.position.x), Math.round(b.entity.position.z), Math.round(b.health ?? 0)]);
  process.send?.({
    type: 'stats',
    worker: cfg.worker,
    bots: bots.length,
    alive,
    deaths: stats.deaths,
    attacks: stats.attacks,
    errors: stats.errors,
    kicks: stats.kicks,
    reconnects: stats.reconnects,
    disconnects: stats.disconnects,
    reached: stats.reached,
    stuck: stats.stuck,
    stuckNames,
    positions,
    elapsed: Math.round((Date.now() - startedAt) / 1000),
  });
}, 2000);

function stop() {
  if (shuttingDown) return;
  shuttingDown = true;
  for (const b of bots) {
    try { clearInterval(b._perf.timer); } catch (_) {}
    try { b.quit(); } catch (_) {}
  }
  setTimeout(() => process.exit(0), 1500);
}
process.on('message', (m) => { if (m && m.type === 'stop') stop(); });
process.on('SIGTERM', stop);
process.on('SIGINT', stop);
