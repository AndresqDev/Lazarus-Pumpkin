#!/usr/bin/env python3
"""Sampler para el baseline PaperSpigot 1.8.8 (sin plugins).

A diferencia de `sampler.py` (Pumpkin), este sampler consulta el comando `/tps`
que trae Spigot/Paper y no intenta leer MSPT: en 1.8.8 no hay un comando
equivalente sin instalar un plugin. La capacidad se decide por TPS y por la
salud del proceso (CPU/RSS), que es lo que nos interesa comparar con B21/B22.

Columnas:
  ts,elapsed,tps,tps1,tps5,tps15,players,paper_cpu,paper_rss_kb,
  clients_25565,load1,memavail_kb
"""

import argparse
import os
import re
import socket
import struct
import subprocess
import sys
import time

CLK_TCK = os.sysconf("SC_CLK_TCK")
START = time.time()
PREV = {}
RAW_TPS_LOGGED = False
RAW_LIST_LOGGED = False


# --------------------------------------------------------------------------
# RCON (mismo protocolo minimo que loadtest/server/rcon.py)
# --------------------------------------------------------------------------
def _packet(rid, typ, body: bytes) -> bytes:
    return struct.pack("<iii", 10 + len(body), rid, typ) + body + b"\x00\x00"


def _read_packet(sock):
    raw = sock.recv(4)
    if len(raw) < 4:
        return None
    (length,) = struct.unpack("<i", raw)
    data = b""
    while len(data) < length:
        chunk = sock.recv(length - len(data))
        if not chunk:
            break
        data += chunk
    return data


def rcon(cmd, host, port, password, timeout=4.0):
    try:
        s = socket.create_connection((host, port), timeout=timeout)
        s.settimeout(1.5)
        s.sendall(_packet(1, 3, password.encode()))
        resp = _read_packet(s)
        if not resp or resp[4:8] != struct.pack("<i", 2):
            s.close()
            return None
        s.sendall(_packet(2, 2, cmd.encode()))
        out = []
        while True:
            try:
                resp = _read_packet(s)
            except socket.timeout:
                break
            if not resp:
                break
            body = resp[8:-2]
            if not body:
                break
            out.append(body.decode("utf-8", "replace"))
        s.close()
        return "\n".join(out).strip()
    except Exception:
        return None


# --------------------------------------------------------------------------
# /proc
# --------------------------------------------------------------------------
def proc_sample(pid):
    """Devuelve (cpu_pct, rss_kb) del proceso Java."""
    if not pid:
        return None, None
    try:
        with open(f"/proc/{pid}/stat", "r", encoding="utf-8") as f:
            data = f.read()
        rp = data.rfind(")")
        fields = data[rp + 2:].split()
        ticks = int(fields[11]) + int(fields[12])
        rss_kb = int(fields[21]) * (os.sysconf("SC_PAGE_SIZE") // 1024)
        now = time.time()
        prev_ticks, prev_ts = PREV.get(pid, (ticks, now))
        PREV[pid] = (ticks, now)
        dt = max(now - prev_ts, 0.001)
        cpu = (ticks - prev_ticks) / CLK_TCK / dt * 100.0
        return round(cpu, 1), rss_kb
    except Exception:
        return None, None


def loadavg():
    try:
        return float(open("/proc/loadavg").read().split()[0])
    except Exception:
        return None


def memavail_kb():
    try:
        for line in open("/proc/meminfo"):
            if line.startswith("MemAvailable:"):
                return int(line.split()[1])
    except Exception:
        pass
    return None


def established(port):
    try:
        out = subprocess.run(
            ["ss", "-Htn", "state", "established", f"( sport = :{port} )"],
            capture_output=True, text=True, timeout=3,
        ).stdout
        return len([ln for ln in out.splitlines() if ln.strip()])
    except Exception:
        return None


# --------------------------------------------------------------------------
# Parseo de salida de Spigot
# --------------------------------------------------------------------------
COLOR = re.compile(r"\x1b\[[0-9;]*m|§[0-9A-FK-ORa-fk-or]")


def clean(text):
    if not text:
        return ""
    # Paper 1.8.8 dibuja la barra de /tps con '*' (no con colores):
    # "TPS from last 1m, 5m, 15m: *20.0, *20.0, *20.0".
    return (
        COLOR.sub("", text)
        .replace("\x1b", "")
        .replace("*", "")
        .replace("#", "")
    )


def parse_tps(raw):
    """Acepta las variantes de /tps de Spigot y Paper 1.8.8."""
    if not raw:
        return None, None, None, None
    text = clean(raw)
    # Formato clasico de Spigot: "Server performance: 20.0"
    m = re.search(r"Server performance:\s*([\d.]+)", text, re.I)
    if m:
        return float(m.group(1)), None, None, None
    # Formato Paper/Spigot moderno: "TPS from last 1m, 5m, 15m: 20.0, 20.0, 20.0"
    m = re.search(
        r"TPS\s+from\s+last\s+1m,\s*5m,\s*15m:\s*"
        r"([\d.]+),\s*([\d.]+),\s*([\d.]+)",
        text, re.I,
    )
    if m:
        a, b, c = (float(x) for x in m.groups())
        return a, a, b, c
    # Fallback: "TPS: 20.0"
    m = re.search(r"\bTPS\b\s*:?\s*([\d.]+)", text, re.I)
    if m:
        return float(m.group(1)), None, None, None
    return None, None, None, None


def parse_online(list_out):
    if not list_out:
        return None
    text = clean(list_out)
    # Paper 1.8.8: "There are 0/1000 players online:"
    m = re.search(r"There are\s+(\d+)\s*/\s*\d+\s+players?\s+online", text, re.I)
    if m:
        return int(m.group(1))
    # Spigot clasico: "There are 0 of a max 1000 players online:"
    m = re.search(r"There are\s+(\d+)\s+of\s+a\s+max", text, re.I)
    if m:
        return int(m.group(1))
    m = re.search(r"There are\s+(\d+)\s+players?\s+online", text, re.I)
    if m:
        return int(m.group(1))
    # Ultimo recurso: contar los nicks listados despues de los dos puntos.
    m = re.search(r"players?\s+online:\s*(.+)$", text, re.I | re.S)
    if m:
        names = [n.strip() for n in m.group(1).split(",") if n.strip()]
        return len(names)
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="sampler.csv")
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=25575)
    ap.add_argument("--pass", dest="password", default="lazarus-test")
    ap.add_argument("--interval", type=float, default=5.0)
    args = ap.parse_args()

    paper_pid = int(os.environ.get("PAPER_PID") or 0) or None
    new_file = not os.path.exists(args.out)
    fh = open(args.out, "a", buffering=1, encoding="utf-8")
    if new_file:
        fh.write(
            "ts,elapsed,tps,tps1,tps5,tps15,players,paper_cpu,"
            "paper_rss_kb,clients_25565,load1,memavail_kb\n"
        )

    def shutdown(*_):
        try:
            fh.flush()
            fh.close()
        finally:
            sys.exit(0)

    import signal
    signal.signal(signal.SIGTERM, shutdown)
    signal.signal(signal.SIGINT, shutdown)

    print(
        f"spigot_sampler: out={args.out} rcon={args.host}:{args.port} "
        f"paper_pid={paper_pid}",
        flush=True,
    )

    while True:
        global RAW_TPS_LOGGED, RAW_LIST_LOGGED
        ts = int(time.time())
        elapsed = int(ts - START)
        raw_tps = rcon("tps", args.host, args.port, args.password)
        tps, tps1, tps5, tps15 = parse_tps(raw_tps)
        if raw_tps and tps is None and not RAW_TPS_LOGGED:
            print(f"WARN /tps no reconocido: {clean(raw_tps)!r}", flush=True)
            RAW_TPS_LOGGED = True
        raw_list = rcon("list", args.host, args.port, args.password)
        if raw_list and not RAW_LIST_LOGGED:
            print(f"INFO /list raw: {clean(raw_list)!r}", flush=True)
            RAW_LIST_LOGGED = True
        players = parse_online(raw_list)
        p_cpu, p_rss = proc_sample(paper_pid)
        row = [
            ts, elapsed,
            "" if tps is None else tps,
            "" if tps1 is None else tps1,
            "" if tps5 is None else tps5,
            "" if tps15 is None else tps15,
            "" if players is None else players,
            "" if p_cpu is None else p_cpu,
            "" if p_rss is None else p_rss,
            "" if established(25565) is None else established(25565),
            "" if loadavg() is None else loadavg(),
            "" if memavail_kb() is None else memavail_kb(),
        ]
        fh.write(",".join(str(x) for x in row) + "\n")
        print(
            f"SAMPLE t={elapsed}s tps={tps} players={players} "
            f"cpu[paper={p_cpu}%] rss[paper={p_rss}KB]",
            flush=True,
        )
        time.sleep(args.interval)


if __name__ == "__main__":
    main()
