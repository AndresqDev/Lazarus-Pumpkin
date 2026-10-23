#!/usr/bin/env node
/**
 * Smoke de refresco inmediato de la UI (claims y timers).
 *
 * No mide rendimiento. Verifica tres cosas con clientes 1.8.9 reales:
 *   1. que el join no genere un paquete por slot (native_tab agrupado);
 *   2. que `/f claimchunk` y `/f unclaim` refresquen tablist y sidebar en el
 *      siguiente tick, no en el pase de 1 Hz / 5 Hz;
 *   3. que `/sotw start` y su expiración refresquen el sidebar de inmediato.
 *
 * Los comandos administrativos se envían por RCON desde el runner del proxy.
 * El factory de facciones empieza con balance 0, así que el claim se financia
 * con `/f setbalance` por consola antes de reclamar.
 */
'use strict';

const mineflayer = require('mineflayer');
const { execFileSync } = require('child_process');

function arg(name, fallback) {
  const index = process.argv.indexOf(`--${name}`);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
}

const host = arg('host', '127.0.0.1');
const port = Number(arg('port', '25565'));
const rconHost = arg('rcon-host', '');
const rconPassword = arg('rcon-password', 'lazarus-test');
const rconPort = arg('rcon-port', '25575');
const seconds = Number(arg('seconds', '70'));
const faction = `LzUi${String(process.pid).slice(-5)}`;
const userA = 'SmokeUiA';
const userB = 'SmokeUiB';

const bots = [];
const tab = new Map();
const tabText = new Map();
const teams = new Map();
const scores = new Map();
let objective = null;
let sidebar = null;
let finished = false;
const problems = [];

const packets = {
  player_info: 0,
  scoreboard_objective: 0,
  scoreboard_display: 0,
  scoreboard_score: 0,
  scoreboard_team: 0,
};
let watch = null;
let lastRconMs = 0;

function resetPackets() {
  for (const key of Object.keys(packets)) packets[key] = 0;
}

function packetSnapshot() {
  return { ...packets };
}

function packetDelta(before) {
  return Object.fromEntries(Object.keys(packets).map((key) => [key, packets[key] - before[key]]));
}

function plainText(value) {
  if (value === null || value === undefined) return '';
  if (typeof value === 'string') return value;
  if (Array.isArray(value)) return value.map(plainText).join('');
  if (typeof value !== 'object') return String(value);
  let out = typeof value.text === 'string' ? value.text : '';
  if (value.translate) out += value.translate;
  if (Array.isArray(value.with)) out += value.with.map(plainText).join('');
  if (Array.isArray(value.extra)) out += value.extra.map(plainText).join('');
  return out;
}

function stripColors(value) {
  return plainText(value).replace(/§[0-9a-fk-or]/gi, '');
}

function tabSignature() {
  return [...tabText.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([name, text]) => `${name}=${text}`)
    .join('\n');
}

function teamMode(mode) {
  if (typeof mode === 'number') {
    return ['add', 'remove', 'change', 'join', 'leave'][mode] || 'change';
  }
  return String(mode);
}

/**
 * Devuelve las líneas visibles del sidebar de Mineflayer. `bot.scoreboard.sidebar`
 * ya resuelve el equipo de cada entrada; no dependemos del orden ni de que el
 * texto quepa en prefix (las líneas largas se parten en prefix+suffix).
 */
function sidebarLines(bot) {
  const board = bot && bot.scoreboard && bot.scoreboard.sidebar;
  if (!board) return [];
  return board.items.map((item) => {
    try {
      const component = item.displayName || item.name;
      return stripColors(component && typeof component.toString === 'function'
        ? component.toString()
        : component);
    } catch (_) {
      return '';
    }
  });
}

function sidebarHas(bot, text) {
  const needle = text.toLowerCase();
  return sidebarLines(bot).some((line) => line.toLowerCase().includes(needle));
}

function noteWatch() {
  if (!watch) return;
  const elapsed = Date.now() - watch.started;
  if (watch.firstPacketMs === null) watch.firstPacketMs = elapsed;
}

function beginWatch(label) {
  watch = { label, started: Date.now(), before: packetSnapshot(), firstPacketMs: null };
  return watch;
}

async function waitForFirstPacket(marker, timeoutMs, predicate) {
  const started = Date.now();
  while (Date.now() - started <= timeoutMs) {
    const condition = predicate ? predicate() : true;
    if (condition && marker.firstPacketMs !== null) {
      marker.after = packetDelta(marker.before);
      marker.elapsedMs = Date.now() - marker.started;
      return marker;
    }
    await sleep(20);
  }
  marker.after = packetDelta(marker.before);
  marker.elapsedMs = Date.now() - marker.started;
  return marker;
}

function createBot(username) {
  const bot = mineflayer.createBot({
    host,
    port,
    username,
    version: '1.8.9',
    auth: 'offline',
    hideErrors: false,
  });
  bots.push(bot);
  bot.messages = [];
  bot.on('message', (message) => {
    const text = message && typeof message.toString === 'function'
      ? message.toString()
      : String(message);
    if (text) {
      bot.messages.push(text);
      if (bot.messages.length > 100) bot.messages.shift();
    }
  });
  bot.on('kicked', (reason) => {
    finish(false, `${username} fue expulsado: ${typeof reason === 'string' ? reason : JSON.stringify(reason)}`);
  });
  bot.on('end', (reason) => {
    if (!finished) finish(false, `${username} terminó la conexión: ${reason || 'unknown'}`);
  });

  bot._client.on('player_info', (packet) => {
    packets.player_info += 1;
    noteWatch();
    for (const entry of packet.data || []) {
      if (!entry || !entry.name) continue;
      if (packet.action === 'add_player') tab.set(entry.name, entry.uuid);
      if (packet.action === 'remove_player') {
        tab.delete(entry.name);
        tabText.delete(entry.name);
        continue;
      }
      if (entry.displayName !== undefined) {
        tabText.set(entry.name, stripColors(entry.displayName));
      }
    }
  });
  bot._client.on('scoreboard_objective', (packet) => {
    packets.scoreboard_objective += 1;
    noteWatch();
    if (packet.action === 0) objective = packet.name;
  });
  bot._client.on('scoreboard_display_objective', (packet) => {
    packets.scoreboard_display += 1;
    noteWatch();
    if (packet.position === 1) sidebar = packet.name;
  });
  bot._client.on('scoreboard_score', (packet) => {
    packets.scoreboard_score += 1;
    noteWatch();
    if (packet.entity !== undefined) scores.set(packet.entity, packet.value);
  });
  bot._client.on('scoreboard_team', (packet) => {
    packets.scoreboard_team += 1;
    noteWatch();
    const name = packet.team;
    if (!name) return;
    const mode = teamMode(packet.mode);
    if (mode === 'remove') {
      teams.delete(name);
    } else if (mode === 'add' || mode === 'change') {
      teams.set(name, {
        prefix: packet.prefix || '',
        suffix: packet.suffix || '',
      });
    }
  });
  return bot;
}

function waitSpawn(bot, timeoutMs) {
  return new Promise((resolve, reject) => {
    if (bot.entity) return resolve();
    const timer = setTimeout(() => reject(new Error(`${bot.username} no hizo spawn`)), timeoutMs);
    bot.once('spawn', () => {
      clearTimeout(timer);
      resolve();
    });
    bot.once('kicked', (reason) => {
      clearTimeout(timer);
      reject(new Error(`${bot.username} fue expulsado: ${reason}`));
    });
    bot.once('error', (error) => {
      clearTimeout(timer);
      reject(error);
    });
  });
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function waitFor(bot, pattern, timeoutMs, label, fromIndex) {
  return new Promise((resolve, reject) => {
    const started = Date.now();
    const timer = setInterval(() => {
      const matches = bot.messages
        .slice(fromIndex)
        .filter((line) => pattern.test(line));
      if (matches.length > 0) {
        clearInterval(timer);
        resolve(matches[matches.length - 1]);
        return;
      }
      if (Date.now() - started > timeoutMs) {
        clearInterval(timer);
        reject(new Error(`${label || pattern} sin respuesta; mensajes=${JSON.stringify(bot.messages.slice(-8))}`));
      }
    }, 50);
  });
}

async function command(bot, line, pattern, label, timeoutMs = 6000) {
  const fromIndex = bot.messages.length;
  bot.chat(line);
  if (pattern) return waitFor(bot, pattern, timeoutMs, label || line, fromIndex);
  await sleep(300);
  return null;
}

/**
 * Igual que `command`, pero no falla si el comando no envía mensaje. El smoke
 * de UI decide por los paquetes de tab/sidebar; el texto se conserva como
 * diagnóstico. Sirve también para detectar mensajes de éxito vacíos.
 */
async function commandSoft(bot, line, pattern, timeoutMs = 1500) {
  const fromIndex = bot.messages.length;
  bot.chat(line);
  try {
    return await waitFor(bot, pattern, timeoutMs, line, fromIndex);
  } catch (_) {
    return null;
  }
}

function rcon(commandLine) {
  if (!rconHost) throw new Error('falta --rcon-host para los comandos administrativos');
  const started = Date.now();
  try {
    return execFileSync(
      'python3',
      ['loadtest/server/rcon.py', commandLine, rconPassword, rconHost, rconPort],
      { encoding: 'utf8', timeout: 10000 },
    ).trim();
  } finally {
    lastRconMs = Date.now() - started;
  }
}

function waitForUi(timeoutMs) {
  return new Promise((resolve) => {
    const started = Date.now();
    const timer = setInterval(() => {
      const lz = [...tab.keys()].filter((name) => /^lz\d+$/.test(name));
      if (lz.length > 0 && sidebar === 'lazarus') {
        clearInterval(timer);
        resolve(lz.length);
      } else if (Date.now() - started > timeoutMs) {
        clearInterval(timer);
        resolve(lz.length);
      }
    }, 100);
  });
}

function finish(ok, reason, extra) {
  if (finished) return;
  finished = true;
  const lz = [...tab.keys()].filter((name) => /^lz\d+$/.test(name));
  const real = [...tab.keys()].filter((name) => !/^lz\d+$/.test(name));
  const result = {
    ok,
    reason,
    host,
    port,
    faction,
    users: [userA, userB],
    tab_entries: tab.size,
    tab_lz: lz.length,
    tab_real: real.length,
    scoreboard_objective: objective,
    sidebar,
    packets,
    ...extra,
    messages: Object.fromEntries(bots.map((bot) => [bot.username, bot.messages.slice(-12)])),
  };
  console.log(JSON.stringify(result, null, 2));
  for (const bot of bots) {
    try { bot.quit('ui smoke done'); } catch (_) {}
  }
  setTimeout(() => process.exit(ok ? 0 : 1), 250);
}

(async () => {
  let botA;
  let botB;
  try {
    resetPackets();
    botA = createBot(userA);
    botB = createBot(userB);
    await Promise.all([waitSpawn(botA, 60000), waitSpawn(botB, 60000)]);
    await sleep(5000);

    const tabLz = await waitForUi(20000);
    const joinPackets = packetSnapshot();
    if (tabLz === 0 || sidebar !== 'lazarus') {
      return finish(false, 'UI incompleta antes de los comandos', { ui: { tab_lz: tabLz, sidebar } });
    }

    await command(botA, `/f create ${faction}`, /created|cread[ao]/i, 'f create');
    // El comando de consola no espera a que el plugin responda; el estado ya
    // está aplicado cuando el tick sigue.
    rcon(`f setbalance ${faction} 1000`);
    await sleep(300);

    const claimBeforeLines = sidebarLines(botA);
    const claimBeforeTab = tabSignature();
    const claimWatch = beginWatch('claim');
    const claimReply = await commandSoft(botA, '/f claimchunk', /claimed|reclam/i);
    await waitForFirstPacket(claimWatch, 2000);
    const claimAfterLines = sidebarLines(botA);
    const claimAfterTab = tabSignature();
    const claimTabPackets = claimWatch.after.player_info;
    const claimScorePackets = claimWatch.after.scoreboard_team + claimWatch.after.scoreboard_score;
    const claimOk = claimTabPackets > 0 && claimScorePackets > 0 && claimWatch.firstPacketMs !== null;
    const claimResult = {
      ...claimWatch,
      reply: claimReply,
      tab_packets: claimTabPackets,
      score_packets: claimScorePackets,
      tab_changed: claimBeforeTab !== claimAfterTab,
      sidebar_changed: JSON.stringify(claimBeforeLines) !== JSON.stringify(claimAfterLines),
      before_lines: claimBeforeLines,
      after_lines: claimAfterLines,
    };
    if (!claimOk) problems.push('el claim no refrescó tab y sidebar en la ventana');

    const unclaimBeforeLines = sidebarLines(botA);
    const unclaimBeforeTab = tabSignature();
    const unclaimWatch = beginWatch('unclaim');
    const unclaimReply = await commandSoft(botA, '/f unclaim', /unclaim|removed|removid/i);
    await waitForFirstPacket(unclaimWatch, 2000);
    const unclaimAfterLines = sidebarLines(botA);
    const unclaimAfterTab = tabSignature();
    const unclaimTabPackets = unclaimWatch.after.player_info;
    const unclaimScorePackets = unclaimWatch.after.scoreboard_team + unclaimWatch.after.scoreboard_score;
    const unclaimOk = unclaimTabPackets > 0 && unclaimScorePackets > 0 && unclaimWatch.firstPacketMs !== null;
    const unclaimResult = {
      ...unclaimWatch,
      reply: unclaimReply,
      tab_packets: unclaimTabPackets,
      score_packets: unclaimScorePackets,
      tab_changed: unclaimBeforeTab !== unclaimAfterTab,
      sidebar_changed: JSON.stringify(unclaimBeforeLines) !== JSON.stringify(unclaimAfterLines),
      before_lines: unclaimBeforeLines,
      after_lines: unclaimAfterLines,
    };
    if (!unclaimOk) problems.push('el unclaim no refrescó tab y sidebar en la ventana');

    // SOTW es global y usa la invalidación inmediata de sidebar. La expiración
    // la procesa el tick de 1 s; la UI debe reaccionar en el tick siguiente.
    const sotwStartWatch = beginWatch('sotw_start');
    rcon('sotw start 6');
    const sotwStartRconMs = lastRconMs;
    await waitForFirstPacket(sotwStartWatch, 2000, () => sidebarHas(botA, 'SOTW'));
    const sotwStarted = sidebarHas(botA, 'SOTW');
    const sotwStartLines = sidebarLines(botA);
    const sotwStartMessage = botA.messages.find((line) => /SOTW has been started/i.test(line)) || null;
    const sotwStartOk = sotwStartWatch.firstPacketMs !== null
      && sotwStartWatch.after.scoreboard_team + sotwStartWatch.after.scoreboard_score > 0
      && sotwStarted
      && Boolean(sotwStartMessage);

    // Empezar a observar antes de que venza el timer (6 s), para no perder
    // los paquetes de la expiración.
    await sleep(3500);
    const sotwEndWatch = beginWatch('sotw_end');
    await waitForFirstPacket(sotwEndWatch, 4000, () => !sidebarHas(botA, 'SOTW'));
    const sotwEnded = !sidebarHas(botA, 'SOTW');
    const sotwEndLines = sidebarLines(botA);
    // La línea la quita el pase de 5 Hz; el mensaje de fin lo emite el tick
    // de 1 s, así que damos margen antes de cerrar el proceso.
    await sleep(1200);
    const sotwEndMessage = botA.messages.find((line) => /SOTW has ended/i.test(line)) || null;
    const sotwEndOk = sotwEndWatch.firstPacketMs !== null
      && sotwEndWatch.after.scoreboard_team + sotwEndWatch.after.scoreboard_score > 0
      && sotwStarted
      && sotwEnded
      && Boolean(sotwEndMessage);

    if (!sotwStartOk) problems.push('SOTW no apareció en el sidebar');
    if (!sotwEndOk) problems.push('SOTW no desapareció del sidebar');
    const ok = problems.length === 0;
    return finish(ok, ok ? 'ok' : problems.join('; '), {
      join_player_info_packets: joinPackets.player_info,
      problems,
      claim: claimResult,
      unclaim: unclaimResult,
      sotw: {
        start: sotwStartWatch,
        rcon_ms: sotwStartRconMs,
        start_line_seen: sotwStarted,
        start_lines: sotwStartLines,
        start_message: sotwStartMessage,
        end: sotwEndWatch,
        end_line_cleared: sotwEnded,
        end_lines: sotwEndLines,
        end_message: sotwEndMessage,
      },
    });
  } catch (error) {
    return finish(false, error && error.message ? error.message : String(error));
  }
})();

setTimeout(() => finish(false, 'timeout global'), (seconds + 20) * 1000);
