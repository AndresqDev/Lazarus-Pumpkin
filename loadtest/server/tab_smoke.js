#!/usr/bin/env node
/**
 * Smoke test mínimo para la tablist nativa y el sidebar de Lazarus.
 *
 * Se conecta como cliente 1.8.9 a Velocity (que traduce a la versión del
 * backend), espera el spawn y durante unos segundos recoge:
 *   - player_info: entradas artificiales `lzNN` de native_tab;
 *   - scoreboard_objective/display/score: sidebar `lazarus`.
 *
 * Sale con 0 solo si encontró al menos una entrada `lz` y el sidebar está
 * visible. No forma parte del loadtest de rendimiento.
 */
'use strict';

const mineflayer = require('mineflayer');

function arg(name, fallback) {
  const index = process.argv.indexOf(`--${name}`);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
}

const host = arg('host', '127.0.0.1');
const port = Number(arg('port', '25565'));
const username = arg('user', 'SmokeTab');
const seconds = Number(arg('seconds', '25'));

const tab = new Map();
let objective = null;
let sidebar = null;
let scorePackets = 0;
let teamPackets = 0;
let finished = false;

const bot = mineflayer.createBot({
  host,
  port,
  username,
  version: '1.8.9',
  auth: 'offline',
  hideErrors: false,
});

function finish(reason) {
  if (finished) return;
  finished = true;

  const lz = [...tab.keys()].filter((name) => /^lz\d+$/.test(name));
  const real = [...tab.keys()].filter((name) => !/^lz\d+$/.test(name));
  const ok = lz.length > 0 && sidebar === 'lazarus';

  console.log(JSON.stringify({
    ok,
    reason,
    host,
    port,
    username,
    tab_entries: tab.size,
    tab_lz: lz.length,
    tab_real: real.length,
    tab_sample_lz: lz.slice(0, 8),
    scoreboard_objective: objective,
    sidebar,
    score_packets: scorePackets,
    team_packets: teamPackets,
  }, null, 2));

  try { bot.quit('smoke done'); } catch (_) {}
  setTimeout(() => process.exit(ok ? 0 : 1), 250);
}

bot._client.on('player_info', (packet) => {
  const action = packet.action;
  const entries = packet.data || [];
  for (const entry of entries) {
    if (!entry || !entry.name) continue;
    if (action === 'add_player') {
      tab.set(entry.name, entry.uuid);
    } else if (action === 'remove_player') {
      tab.delete(entry.name);
    } else if (action === 'update_display_name') {
      // Keep the entry; the display name is rendered by native_tab.
      if (!tab.has(entry.name)) tab.set(entry.name, entry.uuid);
    }
  }
});

bot._client.on('scoreboard_objective', (packet) => {
  if (packet.action === 0) {
    objective = packet.name;
  }
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

bot.once('spawn', () => {
  console.log(`SPAWN ${username} -> ${host}:${port}`);
  setTimeout(() => finish('timer'), seconds * 1000);
});

bot.on('kicked', (reason) => {
  console.error('KICKED', typeof reason === 'string' ? reason : JSON.stringify(reason));
  finish('kicked');
});

bot.on('error', (error) => {
  console.error('ERROR', error && error.stack ? error.stack : error);
});

bot.on('end', (reason) => {
  if (!finished) finish(`end: ${reason || 'unknown'}`);
});

setTimeout(() => finish('timeout'), (seconds + 20) * 1000);
