#!/usr/bin/env node
/**
 * Smoke técnico de la línea de energía de clase en el sidebar.
 *
 * No mide rendimiento. Con un cliente 1.8.9 real:
 *   1. entrega y equipa la armadura dorada completa (Bard);
 *   2. comprueba que la línea de warmup aparece antes de activar;
 *   3. comprueba que, al activarse, aparecen `Class: Bard` y `Energy:`;
 *   4. muestrea la energía dos veces y verifica que la recarga se refleja;
 *   5. quita la armadura y comprueba que la línea activa desaparece.
 *
 * Los comandos administrativos se envían por RCON desde el runner del proxy.
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
const seconds = Number(arg('seconds', '45'));
const username = 'SmokeClass';

// `/item replace entity` writes the armour slot directly and sends the
// equipment update. Unlike Mineflayer's `bot.equip`, it does not depend on a
// window-click transaction, which the proxy/server combination does not
// confirm reliably for this bot.
const armor = [
  { item: 'golden_helmet', slot: 'armor.head' },
  { item: 'golden_chestplate', slot: 'armor.chest' },
  { item: 'golden_leggings', slot: 'armor.legs' },
  { item: 'golden_boots', slot: 'armor.feet' },
];

let bot = null;
let finished = false;
const problems = [];
const packets = { scoreboard_team: 0, scoreboard_score: 0 };
let lastRconMs = 0;

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
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

/**
 * `bot.scoreboard.sidebar` ya resuelve prefix+suffix de cada entrada. No
 * dependemos del orden ni de que el texto quepa entero en el prefix.
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

function energyValue(bot) {
  for (const line of sidebarLines(bot)) {
    const match = line.match(/Energy:\s*([0-9]+(?:\.[0-9]+)?)/i);
    if (match) return Number(match[1]);
  }
  return null;
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

async function waitFor(predicate, timeoutMs, label) {
  const started = Date.now();
  while (Date.now() - started <= timeoutMs) {
    if (predicate()) return { ok: true, elapsed_ms: Date.now() - started };
    await sleep(50);
  }
  return { ok: false, elapsed_ms: Date.now() - started, label };
}

function finish(ok, reason, extra) {
  if (finished) return;
  finished = true;
  const result = {
    ok,
    reason,
    host,
    port,
    user: username,
    sidebar_lines: bot ? sidebarLines(bot) : [],
    packets,
    ...extra,
    messages: bot ? bot.messages.slice(-12) : [],
  };
  console.log(JSON.stringify(result, null, 2));
  try { if (bot) bot.quit('class smoke done'); } catch (_) {}
  setTimeout(() => process.exit(ok ? 0 : 1), 250);
}

(async () => {
  try {
    bot = mineflayer.createBot({
      host,
      port,
      username,
      version: '1.8.9',
      auth: 'offline',
      hideErrors: false,
    });
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
    bot._client.on('scoreboard_team', () => { packets.scoreboard_team += 1; });
    bot._client.on('scoreboard_score', () => { packets.scoreboard_score += 1; });

    await waitSpawn(bot, 60000);
    await sleep(3000);

    const equipped = [];
    for (const { item, slot } of armor) {
      rcon(`item replace entity ${username} ${slot} with minecraft:${item}`);
      equipped.push(slot);
      await sleep(150);
    }
    await sleep(500);

    const warmup = await waitFor(
      () => sidebarHas(bot, 'Class Warmup') && sidebarHas(bot, 'Bard'),
      5000,
      'warmup',
    );
    if (!warmup.ok) problems.push('la línea de warmup de Bard no apareció');

    const active = await waitFor(
      () => sidebarHas(bot, 'Class: Bard') && energyValue(bot) !== null,
      15000,
      'active',
    );
    if (!active.ok) problems.push('la línea activa de Bard/Energy no apareció');

    const before = energyValue(bot);
    const beforePackets = { ...packets };
    await sleep(2200);
    const after = energyValue(bot);
    const energyPackets = {
      scoreboard_team: packets.scoreboard_team - beforePackets.scoreboard_team,
      scoreboard_score: packets.scoreboard_score - beforePackets.scoreboard_score,
    };
    const energyAdvanced = before !== null && after !== null && after > before + 0.5;
    if (!energyAdvanced) problems.push(`la energía no avanzó (${before} -> ${after})`);
    if (energyPackets.scoreboard_team + energyPackets.scoreboard_score === 0) {
      problems.push('la recarga de energía no envió paquetes de sidebar');
    }

    // `/clear` also empties `entity_equipment`, so the server's armour check
    // deactivates the class without another window transaction.
    rcon(`clear ${username}`);
    const deactivated = await waitFor(
      () => !sidebarHas(bot, 'Class: Bard') && !sidebarHas(bot, 'Class Warmup'),
      5000,
      'deactivate',
    );
    if (!deactivated.ok) problems.push('la línea de clase no desapareció al quitar la armadura');

    const ok = problems.length === 0;
    return finish(ok, ok ? 'ok' : problems.join('; '), {
      equipped,
      warmup,
      active,
      energy: {
        before,
        after,
        advanced: energyAdvanced,
        packets: energyPackets,
      },
      deactivated,
      problems,
    });
  } catch (error) {
    return finish(false, error && error.message ? error.message : String(error));
  }
})();

setTimeout(() => finish(false, 'timeout global'), (seconds + 20) * 1000);
