# Benutzerhandbuch – Ping Plotter

Dieses Programm pingt eine Liste von Zieladressen im 2-Sekunden-Takt, zeigt die Ergebnisse in der Konsole an und schreibt optionale Logeinträge.

## Start & Parameter
Das Tool nutzt jetzt benannte Flags.

Beispiele:
- `ping-plotter`  
  Nutzt Standardpfade (`ips.txt`, `result.txt` neben der Binary), läuft unendlich.
- `ping-plotter --duration 120`  
  120 Sekunden Laufzeit, Standardpfade.
- `ping-plotter --ips /pfad/ips.txt --log /pfad/result.txt`  
  Eigene Dateien, unendliche Laufzeit.
- `ping-plotter --ips /pfad/ips.txt --duration 300 --log /pfad/result.txt`  
  Eigene Dateien, 300 Sekunden Laufzeit.

Flags:
- `-d, --duration <sekunden>`: Laufzeit in Sekunden (optional, sonst unendlich).
- `-i, --ips <pfad>`: Pfad zur IP-Liste (optional).
- `-l, --log <pfad>`: Pfad zur Logdatei (optional).

## Dateien & Pfade
- **IP-Liste**: Standard `ips.txt` im Ordner der Binary. Eine IP pro Zeile, leere Zeilen werden ignoriert.
- **Logfile**: Standard `result.txt` im Ordner der Binary. Wird angelegt, falls nicht vorhanden.

## Laufzeitverhalten
- Start richtet sich auf die nächste gerade Sekunde aus, danach alle 2 Sekunden ein Ping pro Ziel.
- Timeout pro Ping: ca. 1900 ms (Prozess wird beendet, wenn länger).
- Konsolenanzeige: Tabelle mit Erfolg/Gesamt und min/avg/max Latenz (ms). Aktualisierung alle 2 Sekunden, Bildschirm wird jeweils neu gezeichnet.
- Logging:
  - In jedem 2-Sekunden-Takt werden unerreichbare Ziele mit Timestamp geloggt (`[YYYY-MM-DD HH:MM:SS] unreachable: ...`).
  - Wenn eine Laufzeit angegeben ist und erreicht wird, wird der letzte Tabellenzustand als “Final state” ins Log geschrieben.

## Voraussetzungen
- Rust-Toolchain zum Bauen (`cargo build --release`).
- System-`ping` muss verfügbar sein:
  - Windows: `ping -n 1 -w 1900`
  - macOS: `ping -c 1 -W 1900`
  - Linux (iputils): `ping -c 1 -W 2`

## Tipps
- Logdatei prüfen, um schnelle Übersicht über nicht erreichbare Ziele zu bekommen.
