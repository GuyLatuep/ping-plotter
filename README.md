# Benutzerhandbuch – Ping Plotter

Dieses Programm pingt eine Liste von Zieladressen im 2-Sekunden-Takt, zeigt die Ergebnisse in der Konsole an und schreibt optionale Logeinträge.

## Start & Parameter
Argumente sind flexibel: erster numerischer Wert = Laufzeit (Sekunden), erster nicht-numerischer Wert = IP-Datei, nächster Wert = Log-Datei.

Beispiele:
- `ping-plotter`  
  Nutzt Standardpfade (`ips.txt`, `result.txt` neben der Binary), läuft unendlich.
- `ping-plotter 120`  
  120 Sekunden Laufzeit, Standardpfade.
- `ping-plotter /pfad/ips.txt /pfad/result.txt`  
  Eigene Dateien, unendliche Laufzeit.
- `ping-plotter /pfad/ips.txt 300 /pfad/result.txt`  
  Eigene Dateien, 300 Sekunden Laufzeit.

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
