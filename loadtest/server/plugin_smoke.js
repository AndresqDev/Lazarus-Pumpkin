#!/usr/bin/env node
/**
 * Smoke funcional del plugin Lazarus completo.
 *
 * No mide rendimiento: valida que, con el perfil HCF activo o desactivado,
 * dos clientes 1.8.9 puedan usar las rutas reales del plugin y que la UI
 * (tablist nativa + sidebar) siga viva:
 *   - /f create, /f invite, /f join, /f show
 *   - /lives check
 *   - /sotw time
 *   - /lazarus
 *   - tablist con entradas `lzNN` y sidebar `lazarus`
 *
 * Sale con 0 solo si todos los pasos reciben respuesta y la UI está presente.
 */
'use strict';

const mineflayer = require('mineflayer');

function arg(name, fallback) {
  const index = process.argv.indexOf(`--${name}`);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
}

const host = arg('host', '127.0.0.1');
const port = Number(arg('port', '25565'));
const seconds = Number(arg('seconds', '35'));
const faction = `LzSmoke${String(process.pid).slice(-5)}`;
const userA = 'SmokePluginA';
const userB = 'SmokePluginB';

const bots = [];
const tab = new Map();
let objective = null;
let sidebar = null;
let scorePackets = 0;
let teamPackets = 0;
let finished = false;

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
      if (bot.messages.length > 80) bot.messages.shift();
    }
  });
  bot.on('kicked', (reason) => {
    finish(false, `${username} fui expulsado: ${typeof reason === 'string' ? reason : JSON.stringify(reason)}`);
  });
  bot.on('end', (reason) => {
    if (!finished) finish(false, `${username} termino la conexion: ${reason || 'unknown'}`);
  });
  bot._client.on('player_info', (packet) => {
    for (const entry of packet.data || []) {
      if (!entry || !entry.name) continue;
      if (packet.action === 'add_player') tab.set(entry.name, entry.uuid);
      if (packet.action === 'remove_player') tab.delete(entry.name);
    }
  });
  bot._client.on('scoreboard_objective', (packet) => {
    if (packet.action === 0) objective = packet.name;
  });
  bot._client.on('scoreboard_display_objective', (packet) => {
    // 1 = sidebar in the 1.8 protocol.
    if (packet.position === 1) sidebar = packet.name;
  });
  bot._client.on('scoreboard_score', (packet) => {
    if (packet.action !== 1) scorePackets += 1;
  });
  bot._client.on('scoreboard_team', () => {
    teamPackets += 1;
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
    }, 100);
  });
}

async function command(bot, line, pattern, label, timeoutMs = 6000) {
  const fromIndex = bot.messages.length;
  bot.chat(line);
  if (pattern) return waitFor(bot, pattern, timeoutMs, label || line, fromIndex);
  await sleep(350);
  return null;
}

async function waitForUi(timeoutMs) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const lz = [...tab.keys()].filter((name) => /^lz\d+$/.test(name));
    if (lz.length > 0 && sidebar === 'lazarus') return lz.length;
    await sleep(250);
  }
  return [...tab.keys()].filter((name) => /^lz\d+$/.test(name)).length;
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
    score_packets: scorePackets,
    team_packets: teamPackets,
    messages: Object.fromEntries(bots.map((bot) => [bot.username, bot.messages.slice(-12)])),
    ...extra,
  };
  console.log(JSON.stringify(result, null, 2));
  for (const bot of bots) {
    try { bot.quit('plugin smoke done'); } catch (_) {}
  }
  setTimeout(() => process.exit(ok ? 0 : 1), 250);
}

(async () => {
  let botA;
  let botB;
  try {
    botA = createBot(userA);
    botB = createBot(userB);
    await Promise.all([waitSpawn(botA, 60000), waitSpawn(botB, 60000)]);
    await sleep(5000);

    const tabLz = await waitForUi(20000);
    const ui = { tab_lz: tabLz, sidebar };
    if (ui.tab_lz === 0 || ui.sidebar !== 'lazarus') {
      return finish(false, 'UI incompleta antes de comandos', { ui });
    }

    await command(botA, `/f create ${faction}`, /created|cread[ao]/i, 'f create');
    await command(botA, `/f invite ${userB}`, /invit/i, 'f invite');
    await command(botB, `/f join ${faction}`, /joined|uni[oó]/i, 'f join');
    await command(botA, `/f show ${faction}`, new RegExp(faction.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'i'), 'f show');
    await command(botA, '/lives check', /lives?/i, 'lives');
    await command(botA, '/sotw time', /sotw|start of the world|not running/i, 'sotw time');
    await command(botA, '/lazarus', /lazarus|commands/i, 'lazarus');

    await sleep(3000);
    const lz = [...tab.keys()].filter((name) => /^lz\d+$/.test(name));
    if (lz.length === 0 || sidebar !== 'lazarus') {
      return finish(false, 'UI desaparecio bajo carga funcional', {
        ui: { tab_lz: lz.length, sidebar },
      });
    }

    return finish(true, 'ok', {
      ui: { tab_lz: lz.length, sidebar },
    });
  } catch (error) {
    return finish(false, error && error.message ? error.message : String(error));
  }
})();

setTimeout(() => finish(false, 'timeout global'), (seconds + 75) * 1000);
