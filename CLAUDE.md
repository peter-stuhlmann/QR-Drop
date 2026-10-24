# CLAUDE.md – QR Drop

> Arbeitstitel: **QR Drop**. Diese Datei ist die verbindliche Projektbeschreibung. Lies sie vollständig, bevor du Code schreibst, und halte dich an die hier getroffenen Entscheidungen. Wenn etwas unklar ist oder eine Entscheidung technisch nicht umsetzbar ist, dokumentiere die Abweichung im Abschnitt „Entscheidungslog“ am Ende.

---

## 1. Vision

Eine Datei vom Computer aufs Handy zu bekommen, soll so einfach sein wie ein Foto zu machen:

**Rechtsklick auf eine Datei → „Per QR-Code senden“ → ein schönes Glas-Fenster zeigt einen QR-Code → Handy-Kamera drüberhalten → Datei ist auf dem Handy.**

- Auf dem Handy wird **keine App** benötigt. Jede Kamera-App bzw. jeder QR-Scanner und jeder Browser genügt (iOS und Android).
- Computer und Handy sind im **selben WLAN**. Es gibt keinen Cloud-Server, kein Konto und keine Internetverbindung ist nötig.
- Zielgruppe sind **nicht-technische Nutzer**. Es gibt niemals ein Terminal, keine Konfigurationsdateien und keine Fachbegriffe in der Oberfläche.
- Plattformen: **Windows 10/11** und **macOS 12+** (Apple Silicon und Intel).
- Unterstützt werden **beliebige Dateien** jeden Typs und jeder Größe, außerdem **Ordner** und **Mehrfachauswahl** (beides wird automatisch als ZIP gestreamt).

## 2. Tech-Stack (festgelegt)

| Bereich       | Wahl                                                                                                             |
| ------------- | ---------------------------------------------------------------------------------------------------------------- |
| App-Framework | **Tauri v2** (aktuelle stabile Version)                                                                          |
| Backend       | **Rust** (stable, Edition 2021), async mit **tokio**                                                             |
| HTTP-Server   | **axum** (auf hyper/tokio), Streaming-Responses                                                                  |
| QR-Erzeugung  | Rust-Crate **`qrcode`** → SVG-String ans Frontend                                                                |
| ZIP-Streaming | **`async_zip`** (oder gleichwertig, Hauptsache streaming und ohne Temp-Datei)                                    |
| Lokale IP     | **`local-ip-address`** (+ eigene Filterlogik, siehe 5.3)                                                         |
| Frontend      | **Vite + TypeScript + React 19**, Styling mit **Tailwind CSS v4** plus eigenen CSS-Variablen für das Glas-Design |
| Animationen   | **Motion** (`motion/react`), sparsam und flüssig                                                                 |
| Icons         | **Lucide**                                                                                                       |
| Tauri-Plugins | `single-instance`, `opener`, `os`, `updater` (vorbereitet, standardmäßig aus)                                    |
| CI/CD         | **GitHub Actions** mit **`tauri-apps/tauri-action`**                                                             |
| Paketierung   | Windows: **NSIS**-Installer (.exe), macOS: **.dmg** (Universal Binary)                                           |

Begründung: Tauri liefert kleine Binaries (~5–10 MB), native Fenster-Effekte (Mica/Acrylic/Vibrancy) und echte Installer. Electron oder Python sind ausdrücklich **nicht** gewünscht.

## 3. Nutzererlebnis im Detail

### 3.1 Installation

- **Windows:** Doppelklick auf `QR-Drop_x.y.z_x64-setup.exe` → Installation pro Benutzer (kein Admin nötig) → der Kontextmenü-Eintrag wird automatisch angelegt. Bei der Deinstallation wird er sauber entfernt.
- **macOS:** `.dmg` öffnen → App in „Programme“ ziehen → beim **ersten Start** richtet die App die Schnelle Aktion selbst ein und zeigt ein kurzes Willkommensfenster („Fertig! Klicke ab jetzt mit der rechten Maustaste auf eine Datei …“) mit einer kleinen Animation, wo der Eintrag zu finden ist.

### 3.2 Benutzung

1. Rechtsklick auf Datei(en) oder einen Ordner → **„Per QR-Code senden“**.
   - Windows 10: direkt im Kontextmenü. Windows 11: unter „Weitere Optionen anzeigen“ (v1, siehe Roadmap).
   - macOS: Kontextmenü → „Schnelle Aktionen“ → „Per QR-Code senden“.
2. Innerhalb von **< 500 ms** erscheint ein kleines, schwebendes Glas-Fenster (always on top, zentriert) mit:
   - Dateiname (mittig gekürzt bei Überlänge), Dateigröße, passendem Dateityp-Icon
   - großem QR-Code (hoher Kontrast, Ruhezone, mindestens 240 × 240 px)
   - Hinweis: „Mit der Handy-Kamera scannen“
   - Countdown-Ring für die Ablaufzeit (Standard 10 Minuten)
   - dezentem Button „Abbrechen“
3. Das Handy öffnet eine **Landingpage** (vom Rechner ausgeliefert, ebenfalls im Glas-Design) mit Dateiname, Größe, Vorschau (bei Bildern) und einem großen Button **„Herunterladen“**. Eine Landingpage statt eines direkten Downloads ist Pflicht, weil iOS sonst manche Dateitypen nur anzeigt statt speichert.
4. Während des Downloads zeigt das Desktop-Fenster live: „Handy verbunden“ → Fortschrittsbalken mit Prozent und MB/s → „Fertig ✓“ mit kurzer Erfolgsanimation.
5. Danach schließt sich das Fenster nach 3 Sekunden automatisch. Der Server wird beendet.

### 3.3 Zustände des Fensters (State Machine)

`preparing → ready → connected → transferring → done`
plus `expired`, `cancelled` und `error` (mit Grund). Jeder Zustand hat ein eigenes, animiertes Layout. Übergänge sind weich (Crossfade + leichtes Scale).

### 3.4 Texte und Sprache

- UI-Sprache folgt der Systemsprache: **Deutsch und Englisch** in v1 (einfaches i18n mit JSON-Dateien).
- Fehlermeldungen sind menschlich formuliert, z. B.: „Dein Handy erreicht den Computer nicht. Sind beide im selben WLAN?“ – niemals Stacktraces oder Fehlercodes in der UI.

### 3.5 Texte, Links und Passwörter (Zwischenablage)

- Einstiege ohne Hintergrundprozess: Windows-Startmenü-Eintrag „QR Drop - Zwischenablage senden“ (`--clipboard`, vom NSIS-Hook angelegt), macOS-Dienst „Text per QR-Code senden“ (Automator, beliebige App, übergibt den Text als `.qrdroptext`-Datei im `$TMPDIR`), in jedem QR-Drop-Fenster Strg+V/⌘V bzw. „Zwischenablage senden“.
- Klassifizierung in Rust (`ShareSource::from_text`): **Link** (`http(s)://…`, ≤ 1200 Zeichen) → der QR-Code enthält die URL direkt, kein Server nötig, kein Ablauf. **Passwort** (8–128 Zeichen, keine Leerzeichen, gemischte Zeichenklassen) → nie im Desktop-Fenster sichtbar, auf dem Handy verdeckt mit „Anzeigen“, Ablauf **2 Minuten**. Sonst **Text** (≤ 1 MB) mit „Kopieren“-Knopf, 10 Minuten.
- Texte werden genau **einmal** ausgeliefert (Landingpage = Abruf), nie als Download-Route, nie geloggt.

### 3.6 Vom Handy empfangen (Handy → Computer)

- Einstiege: Button „Vom Handy empfangen“ in der Drop-Zone, Windows-Startmenü-Eintrag „QR Drop - Vom Handy empfangen“ (`--receive`, vom NSIS-Hook angelegt).
- Das Fenster zeigt einen QR-Code. Das Handy öffnet eine Upload-Seite (`landing/upload.html`, gleiche Glas-Optik) mit „Dateien auswählen“ (Fotos, Kamera, Dateien; Mehrfachauswahl) und einem Textfeld.
- Dateien landen immer im Ordner **„Downloads“** (Nutzerentscheidung, kein Nachfragen). Texte landen in der **Zwischenablage** des Computers.
- Fenster-Zustände: `ready → connected → uploading → received` (plus `expired`, `error`). Die Sitzung bleibt offen, solange das Fenster offen ist; das Handy kann mehrfach senden. „Ordner öffnen“ zeigt die empfangenen Dateien im Explorer/Finder.
- Ablauf: 10 Minuten **ohne Übertragung** (nur echte Datenübertragung verlängert), höchstens **60 Minuten** insgesamt.

## 4. Architektur

```
Kontextmenü (OS)
   │  startet: QR-Drop.exe "<pfad1>" ["<pfad2>" …]   (Win)
   │           open -a "QR Drop" <pfade>             (mac → RunEvent::Opened)
   ▼
Tauri-Prozess (single instance)
   ├─ Rust-Core
   │   ├─ args/            Pfade einsammeln (Debounce 300 ms für Mehrfachauswahl)
   │   ├─ share/           Share-Session: Token, Ablauf, Status, Events
   │   ├─ server/          axum: Landingpage, Download, Fortschritt
   │   ├─ net/             Auswahl der richtigen LAN-IP
   │   ├─ qr/              SVG-Erzeugung
   │   ├─ integration/     Kontextmenü-Setup (mac Quick Action, Win-Checks)
   │   └─ commands.rs      Tauri-Commands + Events ans Frontend
   └─ Frontend (React)     Glas-Fenster, State Machine, Animationen
```

### 4.1 Ein Prozess, mehrere Shares

- `tauri-plugin-single-instance`: Startet der Nutzer einen zweiten Share, geht er an die laufende Instanz. Pro Share öffnet sich ein eigenes Fenster (gestapelt, leicht versetzt). Alle Shares teilen sich **einen** HTTP-Server mit unterschiedlichen Tokens.
- **Mehrfachauswahl unter Windows:** Explorer startet pro Datei einen eigenen Prozess. Die Instanz sammelt alle Pfade, die innerhalb von **300 ms** eintreffen, zu **einem** Share (→ ZIP). Das Fenster öffnet sich dabei **sofort** mit dem ersten Pfad; kommen innerhalb der 300 ms weitere hinzu, wird derselbe Share zum ZIP erweitert (neuer QR-Code im selben Fenster).
- Wird die App ohne Argumente gestartet (Doppelklick auf das App-Icon), öffnet sich ein Fenster mit **Drag-&-Drop-Zone** („Datei hierher ziehen“) sowie einem Link zur Hilfe. Das ist zugleich der Fallback, falls das Kontextmenü fehlt.
- Ist kein Share mehr aktiv und kein Fenster mehr offen, beendet sich der Prozess vollständig. Es gibt in v1 **kein** Tray-Icon und keinen Autostart.

## 5. Rust-Core: Anforderungen

### 5.1 HTTP-Server

- Bindet auf `0.0.0.0` an einen **zufälligen freien Port** im Bereich 49152–65535 (beim ersten Mal merken und später bevorzugt wiederverwenden, damit die Firewall-Freigabe stabil bleibt).
- Routen (alles unter einem zufälligen Token, 128 Bit, URL-safe Base64):
  - `GET /s/{token}` → Landingpage (HTML, inline CSS/JS, keine externen Ressourcen)
  - `GET /s/{token}/file` → Download-Stream
  - `GET /s/{token}/preview` → verkleinerte Bildvorschau (nur bei Bildern ≤ 50 MB, sonst 404)
  - `POST /s/{token}/take` → Text, Link oder Passwort eines Text-Shares (einmalig; die Landingpage selbst enthält nur Metadaten, damit Link-Vorschauen und Prefetch den Text nicht verbrauchen)
  - `GET /s/{token}/check?size=…` → 204 wenn die nächste Datei auf die Platte passt (64 MB Reserve), sonst `507 {"error":"disk_full"}`; die Upload-Seite fragt vor jeder Datei
  - `POST /s/{token}/upload?name=…&i=…&n=…&total=…&offset=…&key=…&size=…&from=…` → Datei vom Handy (roher Body, kein Multipart), nur für Empfangs-Sitzungen; Antwort `201 {"name": "<gespeicherter Name>"}`. **Fortsetzbar:** mit `key` (zufällig, vom Handy) bleibt bei einem Abbruch das Empfangene erhalten, `from` setzt dort fort (falsche Position → `409 {"offset": n}`)
  - `GET /s/{token}/upload?key=…` → `{"offset": n}`: so viele Bytes dieser Datei hat der Computer schon
  - `POST /s/{token}/text` → Text vom Handy in die Zwischenablage (≤ 1 MB), `204`
  - `GET /s/{token}/status` → 204, solange der Code gilt, sonst `404 {"reason": "done" | "expired" | "unknown"}` (ohne Events; damit erkennt die Landingpage einen abgeschlossenen Download und blendet „Erneut herunterladen“ aus)
  - alles andere → 404 ohne Details
- Download:
  - `Content-Disposition: attachment; filename="…"; filename*=UTF-8''…` (RFC 6266/5987, Umlaute und Emojis korrekt)
  - korrekter `Content-Type` per Endung (`mime_guess`), Fallback `application/octet-stream`
  - `Content-Length` bei Einzeldateien; **HTTP-Range-Support** (wichtig für iOS/Videos und für fortsetzbare Downloads)
  - Streaming in 64-KiB-Chunks. Die Datei wird **nie** komplett in den RAM geladen. Dateien > 4 GB müssen funktionieren.
  - Ordner und Mehrfachauswahl: ZIP **on the fly** streamen (STORE ohne Kompression für schon komprimierte Typen, sonst DEFLATE). Name: `<Ordnername>.zip` bzw. `QR-Drop_<Datum>.zip`.
- Fortschritt: Bytes zählen und per Tauri-Event `share://progress` (max. 10×/s) ans Frontend senden, außerdem `share://connected`, `share://done`, `share://error`.

### 5.2 Sicherheit

- Nur Clients aus **privaten Adressbereichen** (RFC 1918, Link-Local, IPv6 ULA) werden bedient, alle anderen erhalten 403.
- Das Token ist das Geheimnis: keine Verzeichnisauflistung, kein Pfad aus der URL wird je auf das Dateisystem gemappt. Der Server kennt nur die Datei(en) der Session.
- **Ablauf:** Standard 10 Minuten, danach ist das Token ungültig.
- **Einmal-Download:** Standardmäßig wird nach dem ersten **vollständigen** Download beendet. Abgebrochene Downloads oder Range-Requests zählen nicht.
- Kein Logging von Dateiinhalten. Logs (nur lokal, rotierend) enthalten keine vollständigen Pfade im Release-Build.
- **Empfangen (Handy → Computer):** Nur Empfangs-Tokens dürfen schreiben. Dateinamen vom Handy sind nicht vertrauenswürdig: nur der letzte Namensbestandteil, verbotene Zeichen ersetzt, keine versteckten Dateien, keine Windows-Gerätenamen (`CON`, `NUL` …), keine Bidi-Steuerzeichen, max. 200 Byte (UTF-8). Nichts wird überschrieben (`Name (2).jpg`). Uploads werden in eine versteckte `.qrdrop-*.part`-Datei gestreamt und erst nach vollständigem Empfang umbenannt; abgebrochene Uploads bleiben zum Fortsetzen liegen, bis das Fenster schließt (Reste älter als 24 h werden beim Start entfernt). Empfangene Dateien tragen die Herkunftsmarkierung des Systems (Windows Mark-of-the-Web, macOS Quarantäne). Texte werden nie geloggt; als Passwort erkannte Texte erscheinen im Fenster nur maskiert.
- **Härtung:** max. 64 gleichzeitige Verbindungen, 15 s für die Request-Header, 60 s Leerlauf; `Host`-Header muss eine IP oder `localhost` sein (DNS-Rebinding); Landingpages mit Hash-CSP ohne `unsafe-inline`.
- Optional für später: PIN-Abfrage auf der Landingpage.

### 5.3 Richtige IP finden

Viele Rechner haben mehrere Interfaces (VPN, Docker, Hyper-V, VirtualBox, WSL). Vorgehen:

1. Alle IPv4-Interfaces sammeln.
2. Virtuelle oder VPN-Adapter anhand der Namen ausschließen (`vEthernet`, `VirtualBox`, `VMware`, `docker`, `utun`, `tun`, `tap`, `WSL`, `Tailscale`, `ZeroTier` …).
3. Priorität: WLAN > Ethernet > Rest. Bevorzugt werden `192.168.x.x`, dann `10.x.x.x`, dann `172.16–31.x.x`.
4. Gibt es mehrere Kandidaten, zeigt das Fenster unter dem QR-Code einen unauffälligen Umschalter („Anderes Netzwerk?“), der den QR-Code mit der anderen IP neu erzeugt.
5. Wird keine LAN-IP gefunden, erscheint der Zustand `error` mit: „Kein WLAN gefunden. Verbinde den Computer mit dem WLAN.“

### 5.4 Firewall

- **Windows:** Der NSIS-Installer legt per `netsh advfirewall` eine **eingehende Regel** für die App-EXE an, nur für das **private** Profil (benötigt einmalig UAC – akzeptabel. Falls der Nutzer ablehnt, greift später der normale Windows-Dialog). Die Deinstallation entfernt die Regel.
- **macOS:** Beim ersten Share erscheint der System-Dialog „Eingehende Verbindungen erlauben?“. Das Willkommensfenster weist vorher darauf hin („Bitte auf ‚Erlauben‘ klicken“).
- Erkennung: Gibt es nach 60 s im Zustand `ready` keinen einzigen Request, erscheint ein dezenter Hinweis mit den häufigsten Ursachen (Gäste-WLAN, Firewall, anderes Netz).

## 6. Kontextmenü-Integration

### 6.1 Windows (NSIS-Hooks)

- Über `bundle.windows.nsis.installerHooks` (`src-tauri/windows/hooks.nsh`) mit den Makros `NSIS_HOOK_POSTINSTALL` und `NSIS_HOOK_POSTUNINSTALL`.
- Registry (HKCU, kein Admin):
  - `HKCU\Software\Classes\*\shell\QRDrop` → `(Default)` = „Per QR-Code senden“, `Icon` = `"<install>\QR Drop.exe",0`, `MultiSelectModel` = `Player`
  - `HKCU\Software\Classes\*\shell\QRDrop\command` → `"<install>\QR Drop.exe" "%1"`
  - dasselbe für `HKCU\Software\Classes\Directory\shell\QRDrop` (Ordner)
- Texte des Eintrags lokalisiert (de/en) nach Installer-Sprache.
- **Roadmap v2:** Top-Level-Eintrag im neuen Windows-11-Menü über `IExplorerCommand` + Sparse Package (MSIX-Identität). Nicht in v1.

### 6.2 macOS (Schnelle Aktion)

- Das Bundle deklariert in `src-tauri/Info.plist` `CFBundleDocumentTypes` mit `LSItemContentTypes` = `public.item` und `public.folder`, Rolle `Viewer`, `LSHandlerRank` = `Alternate`. Damit nimmt die App jede Datei an, ohne sich als Standard-App aufzudrängen. Das ermöglicht auch „Öffnen mit → QR Drop“ und Drag aufs Dock-Icon.
- Empfang der Pfade über `tauri::RunEvent::Opened { urls }`.
- Eine fertige Automator-Schnelle-Aktion `Per QR-Code senden.workflow` liegt als Ressource im Bundle (`src-tauri/resources/macos/`). Sie führt aus: `open -a "QR Drop" "$@"` und akzeptiert „Dateien oder Ordner“ im „Finder“.
- Beim ersten Start (und bei jedem Start prüfen, ob noch vorhanden und aktuell) kopiert die App den Workflow nach `~/Library/Services/` und aktualisiert danach den Services-Cache (`/System/Library/CoreServices/pbs -update`). Kein Terminal für den Nutzer – das passiert intern.
- Alternative, falls die Quick Action unzuverlässig ist: native `NSServices`-Registrierung über `objc2`. Im Entscheidungslog festhalten, falls gewechselt wird.

## 7. Design: „Liquid Glass“

Ziel ist ein **absolut modernes, hochwertiges Glas-Design**, das sich auf macOS (Tahoe/Liquid-Glass-Ästhetik) und Windows 11 (Mica/Acrylic, Fluent) gleichermaßen nativ anfühlt.

### 7.1 Fenster

- Tauri-Window: `transparent: true`, `decorations: false`, `shadow: true`, `alwaysOnTop: true`, `resizable: false`, `center: true`, ca. **380 × 540 px**, `skipTaskbar: false`.
- macOS: `macOSPrivateApi: true` (nötig für Transparenz). Native Vibrancy über Tauri-`windowEffects`: `hudWindow` (Fallback `popover`), Zustand `followsWindowActiveState`, Eckenradius 20.
- Windows 11: `windowEffects` `mica` (Fallback `acrylic`). **Windows 10:** halbtransparentes CSS-Glas als Fallback (kein natives Mica).
- Eigene Titelleiste: unsichtbare Drag-Region (`data-tauri-drag-region`) oben und ein runder Schließen-Button (macOS links, Windows rechts – per `@tauri-apps/plugin-os` erkennen).

### 7.2 Gestaltungsregeln

- **Schichten:** natives Vibrancy als Basis → darüber Karten mit `backdrop-filter: blur(24px) saturate(180%)`, Hintergrund `rgba(255,255,255,0.10–0.18)` (Dark) bzw. `rgba(255,255,255,0.55)` (Light).
- **Kanten:** 1 px Innenkontur mit Verlauf (oben hell, unten dunkler) für den „Glasrand“, zusätzlich ein feiner Specular-Highlight-Verlauf oben.
- **Ecken:** 20 px am Fenster, 16 px an Karten, Buttons als Pills.
- **QR-Code-Karte:** Der QR-Code selbst sitzt auf **reinem Weiß** mit schwarzen Modulen (Scanbarkeit hat Vorrang vor Ästhetik!). Die Karte drumherum ist Glas mit einem weichen, langsam rotierenden Farbverlauf-Glow (conic-gradient, sehr dezent).
- **Farben:** neutral, mit einer Akzentfarbe (Standard: kühles Blau-Violett, z. B. `oklch(0.68 0.17 275)`). Unter Windows optional die System-Akzentfarbe übernehmen.
- **Typografie:** Systemschriften (`-apple-system`/SF Pro bzw. `Segoe UI Variable`), Dateiname 17 px semibold, Rest 13–14 px. Zahlen mit `tabular-nums`.
- **Hell/Dunkel:** folgt dem System automatisch, alle Farben als CSS-Variablen auf `:root` mit Dark-Variante.
- **Bewegung:** Fenster erscheint mit Scale 0.96 → 1 und Fade (220 ms, ease-out-expo). QR-Code baut sich mit kurzem Shimmer auf. Erfolgszustand mit Häkchen-Animation (SVG stroke-draw) und kleinem Partikel-/Glow-Puls. `prefers-reduced-motion` wird respektiert.
- **Barrierefreiheit:** Kontrast WCAG AA für alle Texte trotz Glas, Fokus-Ringe sichtbar, Esc = Abbrechen, Screenreader-Labels.

### 7.3 Landingpage (Handy)

- Gleiche Designsprache: großflächiger, weicher Farbverlauf als Hintergrund, eine zentrale Glas-Karte mit Icon/Vorschau, Name, Größe und großem Download-Button (mindestens 52 px hoch, daumenfreundlich).
- Nach dem Tippen: Hinweistext passend zum Gerät (per User-Agent), z. B. iOS: „Die Datei findest du in der App ‚Dateien‘ unter ‚Downloads‘.“
- Eine einzige HTML-Datei ohne externe Requests, < 30 KB, mit `include_str!` ins Binary eingebettet. Funktioniert in Safari (iOS 15+), Chrome und Samsung Internet.

## 8. Projektstruktur

```
qr-drop/
├─ CLAUDE.md
├─ README.md                      # Nutzer-Anleitung (de/en), Screenshots
├─ package.json / pnpm-lock.yaml
├─ vite.config.ts
├─ index.html
├─ src/                           # Frontend
│  ├─ main.tsx
│  ├─ App.tsx                     # Router: share-window | welcome | dropzone
│  ├─ state/shareMachine.ts       # Zustandsautomat
│  ├─ components/                 # GlassCard, QrCard, ProgressRing, Countdown, FileBadge …
│  ├─ styles/tokens.css           # Design-Tokens, Light/Dark
│  ├─ i18n/{de,en}.json
│  └─ lib/tauri.ts                # typisierte Commands/Events
├─ landing/                       # Handy-Seiten: index.html, upload.html + common.css/js (beim Ausliefern eingesetzt)
├─ src-tauri/
│  ├─ Cargo.toml
│  ├─ tauri.conf.json
│  ├─ capabilities/               # Rechte pro Fenster: default, session, dropzone, welcome
│  ├─ Info.plist                  # macOS: CFBundleDocumentTypes
│  ├─ windows/hooks.nsh           # Registry + Firewall
│  ├─ resources/macos/Per QR-Code senden.workflow/
│  ├─ icons/
│  └─ src/
│     ├─ main.rs / lib.rs
│     ├─ app_core.rs  args.rs  batch.rs  commands.rs  disk.rs  handoff.rs  net.rs  qr.rs
│     ├─ sessions.rs  settings.rs  startup_error.rs  ui.rs  update.rs
│     ├─ share/{mod, manager, progress, scan, source, text}.rs
│     ├─ server/{mod, conn, routes, page, download, stream, zip}.rs
│     ├─ server/receive/{mod, names, store, uploads}.rs
│     └─ integration/{mod, macos, services, windows}.rs
└─ .github/workflows/
   ├─ ci.yml                      # bei jedem Push/PR
   └─ release.yml                 # bei Tag v*
```

## 9. GitHub Actions

### 9.1 `ci.yml` (Push und Pull Request)

- Matrix: `windows-latest`, `macos-latest`.
- Schritte: Checkout → pnpm + Node LTS → Rust stable (`dtolnay/rust-toolchain`) → `Swatinem/rust-cache` → `pnpm install --frozen-lockfile` → `pnpm lint` + `pnpm typecheck` → `cargo fmt --check` → `cargo clippy -- -D warnings` → `cargo test` → `pnpm tauri build --debug` (nur als Smoke-Test).

### 9.2 `release.yml` (Tag `v*.*.*`)

- `tauri-apps/tauri-action` mit Matrix:
  - `macos-latest` mit `--target universal-apple-darwin` (vorher `rustup target add aarch64-apple-darwin x86_64-apple-darwin`)
  - `windows-latest` (x64; arm64 als optionaler zweiter Eintrag)
- Erstellt einen **GitHub-Release-Entwurf** mit `.dmg` und `-setup.exe` sowie automatisch generierten Release Notes.
- **Signierung optional über Secrets** – der Build muss auch **ohne** Secrets durchlaufen (dann unsigniert):
  - macOS: `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` (Notarisierung)
  - Windows: Code-Signing-Zertifikat bzw. Azure Trusted Signing (Secrets dokumentieren)
  - Updater: `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- Die Version wird **nur** in `package.json` gepflegt. `tauri.conf.json` liest sie von dort, damit es keine doppelte Pflege gibt.

## 10. Tests und Qualität

- **Rust-Unit-Tests:** IP-Auswahl (mit Fake-Interfaces), Token-Validierung, Filename-Encoding (Umlaute, Emojis, Sonderzeichen, sehr lange Namen), Range-Parsing, Ablauf und Einmal-Download.
- **Integrationstests:** Server starten, per `reqwest` Datei laden, Hash vergleichen, inklusive Range-Requests, ZIP-Inhalt prüfen und 4-GB+-Sparse-Datei (Streaming ohne RAM-Spitze).
- **Frontend:** Vitest für die State Machine. Komponenten visuell über eine einfache Storybook-artige Demo-Seite im Dev-Modus (`?demo=1`), auf der jeder Zustand angezeigt wird.
- **Manuelle Testmatrix (vor jedem Release):** iPhone (Kamera + Safari), Android (Google-Kamera/Lens + Chrome, Samsung-Kamera + Samsung Internet) × Windows 11, Windows 10, macOS (Apple Silicon). Dateitypen: JPG, HEIC, PDF, MP4 (> 1 GB), DOCX, ZIP, Datei ohne Endung, Ordner, Mehrfachauswahl, Dateiname mit Emoji.
- Code-Stil: `rustfmt`, `clippy` ohne Warnungen, ESLint + Prettier, TypeScript `strict`. Kein `unwrap()` im Produktionscode außerhalb von Tests.

## 11. Meilensteine

1. **M1 – Kern:** Tauri-Grundgerüst, Pfad per Argument, axum-Server, Token, Download mit Range, QR-Code im schlichten Fenster. → Datei kommt aufs Handy.
2. **M2 – Glas-UI:** Design-Tokens, natives Vibrancy/Mica, alle Zustände animiert, Live-Fortschritt, Landingpage.
3. **M3 – Integration:** NSIS-Hooks (Registry + Firewall), macOS-Quick-Action-Setup, Single-Instance, Mehrfachauswahl und Ordner als ZIP, Willkommensfenster, Drop-Zone.
4. **M4 – Release:** GitHub Actions (CI + Release), Icons, README mit Screenshots, i18n, Testmatrix durchlaufen.
5. **Roadmap (nach v1):** Windows-11-Top-Level-Menü, PIN-Schutz, Zielordner wählbar bzw. „Vom Handy hierher empfangen“ per Ordner-Rechtsklick, mehrere Downloads erlauben, Tray-Modus.

## 12. Definition of Done (v1)

- Ein Nutzer ohne Technikkenntnisse kann die App installieren und eine Datei aufs Handy übertragen, **ohne irgendwo etwas eintippen zu müssen**.
- Funktioniert mit jeder Datei, mit Ordnern und mit Mehrfachauswahl, mit iOS und Android, ohne App auf dem Handy.
- Das Kontextmenü erscheint nach der Installation zuverlässig auf Windows 10/11 und macOS 12+.
- Zeit von Rechtsklick bis sichtbarem QR-Code < 500 ms (warmer Start) bzw. < 1,5 s (kalter Start).
- Installer ≤ 15 MB. Leerlauf-RAM ≤ 80 MB.
- CI ist grün, und ein Tag erzeugt automatisch beide Installer.

## 13. Arbeitsweise für Claude

- Arbeite meilensteinweise und halte nach jedem Meilenstein eine lauffähige App bereit.
- Bevorzuge kleine, gut benannte Module statt großer Dateien.
- Füge keine Abhängigkeiten hinzu, die nicht nötig sind. Begründe jede neue Crate bzw. jedes neue npm-Paket kurz im Commit-Text.
- Prüfe Tauri-v2-APIs gegen die aktuelle offizielle Dokumentation, statt sie aus dem Gedächtnis zu übernehmen (Config-Schlüssel ändern sich).
- Rechte in `capabilities/` minimal halten. Kein uneingeschränkter Dateisystem- oder Shell-Zugriff im Frontend.
- Commits im Format Conventional Commits (`feat:`, `fix:`, `ci:` …).

## 14. Entscheidungslog

| Datum      | Entscheidung                                                                                                                                                                                                                                                            | Grund                                                                                                                                                                                                    |
| ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 2026-10-20 | Tauri v2 + Rust + React, GitHub Actions für Builds                                                                                                                                                                                                                      | Kleine App, native Glas-Effekte, echte Installer, Mac-Build ohne eigenen Mac                                                                                                                             |
| 2026-10-20 | Landingpage statt Direkt-Download                                                                                                                                                                                                                                       | iOS speichert sonst manche Typen nicht zuverlässig                                                                                                                                                       |
| 2026-10-20 | Windows-11-Top-Level-Menü erst in v2                                                                                                                                                                                                                                    | Erfordert Sparse Package/MSIX, hoher Aufwand                                                                                                                                                             |
| 2026-10-21 | Eigener Streaming-ZIP-Writer (`flate2` + `crc32fast`) statt `async_zip`                                                                                                                                                                                                 | Volle Kontrolle über ZIP64 und Data Descriptors, läuft in `spawn_blocking`, Inhalt per Test mit dem `zip`-Crate verifiziert                                                                              |
| 2026-10-21 | Landingpage als handgeschriebene Einzeldatei (`landing/index.html`) statt eigener Vite-Stufe                                                                                                                                                                            | Keine zusätzliche Build-Abhängigkeit (heute zwei Seiten mit gemeinsamem `common.css`/`common.js`, jeweils < 30 KB ausgeliefert); Daten werden serverseitig als JSON eingesetzt                           |
| 2026-10-21 | `tauri-plugin-opener` nicht registriert (seit 2026-10-22 nur als Crate für „Ordner öffnen“), Hilfe ist in der App; dafür `tauri-plugin-dialog` für „Datei auswählen …“                                                                                                  | Kein externer Link nötig; der Dialog läuft in Rust, das Frontend braucht keine Dateisystem-Rechte                                                                                                        |
| 2026-10-21 | ~~Updater noch nicht registriert~~ (überholt, siehe 2026-10-24)                                                                                                                                                                                                         | Braucht ein Signatur-Schlüsselpaar; `release.yml` reicht `TAURI_SIGNING_*` bereits durch                                                                                                                 |
| 2026-10-21 | macOS-Fenstereffekt: Liquid Glass (macOS 26) mit Fallback `hudWindow`                                                                                                                                                                                                   | Tauri 2.12 unterstützt beides in einer Effektliste                                                                                                                                                       |
| 2026-10-21 | Schnelle Aktion startet `open -b app.qrdrop.desktop` (Fallback `open -a "QR Drop"`)                                                                                                                                                                                     | Unabhängig vom App-Namen/Ort                                                                                                                                                                             |
| 2026-10-21 | Windows: Release-Builds reparieren einen fehlenden Kontextmenü-Eintrag (HKCU, via `reg.exe`)                                                                                                                                                                            | Robust gegen Registry-Cleaner, kein zusätzlicher Crate                                                                                                                                                   |
| 2026-10-21 | Figma-Variablen als Collections „Theme/Dark“ und „Theme/Light“ mit gleichen Namen                                                                                                                                                                                       | Figma-Starter-Plan erlaubt nur einen Modus pro Collection                                                                                                                                                |
| 2026-10-21 | TypeScript 6 statt 7                                                                                                                                                                                                                                                    | `typescript-eslint` unterstützt TS 7 noch nicht                                                                                                                                                          |
| 2026-10-21 | Text/Link/Passwort-Freigabe ohne Tray, Autostart oder globales Tastenkürzel                                                                                                                                                                                             | Nutzerentscheidung: kein Hintergrundprozess; Einstieg über Startmenü, macOS-Dienst und Strg+V/⌘V                                                                                                         |
| 2026-10-21 | `tauri-plugin-clipboard-manager` für das Lesen der Zwischenablage in Rust                                                                                                                                                                                               | Funktioniert auch ohne Fokus/Paste-Event im Webview; Frontend braucht keine Clipboard-Rechte                                                                                                             |
| 2026-10-21 | macOS-Textdienst übergibt Text als `.qrdroptext`-Datei (umask 077, `$TMPDIR`), App liest und löscht sie                                                                                                                                                                 | `open` kann keine Texte übergeben; Datei-Weg nutzt das vorhandene `RunEvent::Opened`                                                                                                                     |
| 2026-10-21 | Keine Windows-Jump-List                                                                                                                                                                                                                                                 | Startmenü-Verknüpfung lässt sich anheften und braucht keinen zusätzlichen Code                                                                                                                           |
| 2026-10-22 | Landingpage fragt nach dem Download `/status` ab und zeigt dann „Fertig“ statt „Erneut herunterladen“                                                                                                                                                                   | Nach dem Einmal-Download ist der Code verbraucht und QR Drop beendet sich; der Button lief sonst ins Leere. Bei abgebrochenem Download bleibt er nutzbar                                                 |
| 2026-10-22 | Handy → Computer: Upload als roher Request-Body pro Datei (XHR), kein Multipart                                                                                                                                                                                         | Server streamt direkt auf die Platte, Fortschritt pro Datei auf dem Handy, keine zusätzliche Crate                                                                                                       |
| 2026-10-22 | Empfangene Dateien immer in „Downloads“                                                                                                                                                                                                                                 | Nutzerentscheidung: am einfachsten für Nicht-Techniker, „Ordner öffnen“ zeigt sie danach an                                                                                                              |
| 2026-10-22 | „Ordner öffnen“ über `tauri_plugin_opener::reveal_items_in_dir` aus Rust (Plugin nicht registriert)                                                                                                                                                                     | Crate war schon Abhängigkeit; das Frontend bekommt keine Datei- oder Shell-Rechte, Pfade kommen nie aus der UI                                                                                           |
| 2026-10-22 | `rust-version` 1.83                                                                                                                                                                                                                                                     | `io::ErrorKind::StorageFull`/`FileTooLarge` für verständliche Meldungen bei voller Platte bzw. FAT32-Grenze                                                                                              |
| 2026-10-24 | Mehrfachauswahl: erstes Fenster sofort öffnen, Share nach der 300-ms-Sammelzeit ggf. zum ZIP erweitern (`BatchSlot`, Scan-Generationen)                                                                                                                                 | Gemessen: Warmstart 0,6 s → 0,2 s, Kaltstart 1,0 s → 0,45 s (Vorgabe < 0,5 s / < 1,5 s). Log-Zeile „Fenster bereit nach … ms“ zur Kontrolle                                                              |
| 2026-10-24 | Glow dreht in 4 Schritten pro Sekunde (`steps(96)`), Countdown-Ring ohne CSS-Transition                                                                                                                                                                                 | Jeder Frame musste Glow und Glas-Karte neu weichzeichnen: QR-Fenster im Leerlauf 350 → 31 ms CPU pro Sekunde                                                                                             |
| 2026-10-24 | WebView2 mit GPU (Standard) statt `--disable-gpu`; RAM-Ziel ≤ 80 MB nicht erreichbar                                                                                                                                                                                    | Gemessen ≈ 190 MB privat (WebView2: GPU-Prozess ~90 MB). Ohne GPU 113 MB, aber 4× CPU-Last; die App beendet sich nach jeder Übertragung ohnehin                                                          |
| 2026-10-24 | Empfangen: freier Platz wird vor jeder Datei geprüft (`/check`), „Abbrechen“ stoppt einen laufenden Upload; `windows-sys`/`libc` direkt deklariert                                                                                                                      | Beide Crates waren schon im Abhängigkeitsbaum (keine neuen Crates); voller Datenträger zeigt sofort eine klare Meldung statt am Ende abzubrechen                                                         |
| 2026-10-24 | Auto-Updater aktiv (`tauri-plugin-updater`, Befehle in Rust): Prüfung nur in der Drop-Zone, Installation nur per Klick und nie während einer Übertragung; Windows `installMode: passive`                                                                                | Fehlerbehebungen ohne Neuinstallation; die Prüfung ist still und optional (offline: nichts). Ersetzt den Eintrag „Updater noch nicht registriert“. Installer 1,8 → 2,4 MB (HTTPS-Client)                 |
| 2026-10-24 | Updater-Artefakte (`createUpdaterArtifacts`) nur im Release-Workflow und nur mit Secret `TAURI_SIGNING_PRIVATE_KEY`; privater Schlüssel liegt außerhalb des Repos (`~/.tauri/qr-drop.key`), Update-Adresse mit Platzhalter `OWNER`                                      | Lokale Builds und Releases ohne Secrets laufen weiter durch (§9.2). Echt getestet: signiertes 0.1.1 gefunden, geladen, Signatur geprüft; manipulierte Datei abgelehnt                                    |
| 2026-10-24 | Fortsetzbare Uploads Handy → Computer: Schlüssel pro Datei, Teil-Datei bleibt bei Abbruch erhalten, Handy setzt automatisch fort (Pausen bis ~2 min, sofort beim Entsperren)                                                                                            | Große Videos starteten nach Sperrbildschirm/WLAN-Wechsel von vorn. Schlüssel vom Handy wird nie zum Dateinamen; Schließen des Fensters verwirft Teil-Dateien                                             |
| 2026-10-24 | macOS-Dienste werden in der Systemsprache installiert (Name + Menütitel, alte Sprachvariante wird entfernt) statt Lokalisierungsdateien im Workflow                                                                                                                     | Verlässlich auch für Automator-Workflows; Logik plattformunabhängig in `integration/services.rs` mit Tests                                                                                               |
| 2026-10-24 | Neuer macOS-Dienst „Vom Handy empfangen“ (ohne Eingabe, App-Menü → Dienste) übergibt eine leere `.qrdropreceive`-Datei                                                                                                                                                  | Gleiches Muster wie der Textdienst; `open` kann laufenden Apps keine Argumente übergeben                                                                                                                 |
| 2026-10-24 | Eigene hyper-Accept-Schleife statt `axum::serve`: Header-Timeout 15 s, Leerlauf 60 s, max. 64 Verbindungen, 5 s Nachlauf beim Beenden                                                                                                                                   | `axum::serve` hat keine Timeouts; ein Gerät im WLAN konnte sonst beliebig viele Verbindungen offen halten. hyper/hyper-util waren schon im Abhängigkeitsbaum                                             |
| 2026-10-24 | Text-Shares: Landingpage enthält nur Metadaten, der Text kommt einmalig per `POST …/take`                                                                                                                                                                               | Link-Vorschauen, Prefetch und Neuladen verbrauchten sonst den Einmal-Text oder legten ihn im HTML-Cache ab                                                                                               |
| 2026-10-24 | `/status` nennt den Grund (`done`, `expired`, `unknown`)                                                                                                                                                                                                                | Die Seite kann „Fertig“, „Abgelaufen“ und „Unbekannt“ unterscheiden statt pauschal „nicht mehr gültig“                                                                                                   |
| 2026-10-24 | Empfangs-Sitzung: 10 min Pause, verlängert nur durch echte Datenübertragung, harte Obergrenze 60 min                                                                                                                                                                    | Nutzerentscheidung im Audit: Status-Abfragen allein halten den Code nicht ewig offen                                                                                                                     |
| 2026-10-24 | Shares laufen während einer Übertragung nie ab; ein Download zählt, wenn Range-Requests die Datei vollständig abdecken (30 s Nachlauf für späte Teil-Requests)                                                                                                          | iOS lädt Videos in vielen Range-Teilen; vorher endete der Share mitten im Download oder zählte nie als fertig                                                                                            |
| 2026-10-24 | Uploads: neuere Verbindung mit gleichem Schlüssel übernimmt (Generation), fertige Schlüssel werden mit `done`/201 beantwortet, max. 64 offene Teil-Dateien (`429 too_many`), unvollständiger Body → `400 incomplete`                                                    | Hängende alte Verbindung nach WLAN-Wechsel blockierte die Fortsetzung; doppelte Abschlüsse erzeugten `Name (2)`                                                                                          |
| 2026-10-24 | Zielname wird atomar reserviert (leere Datei exklusiv anlegen, dann die Teil-Datei darüber umbenennen) statt vorher geprüft                                                                                                                                             | Zwei gleichzeitige Uploads gleichen Namens konnten sich sonst überschreiben; Symlinks werden nicht verfolgt                                                                                              |
| 2026-10-24 | Empfangene Dateien mit Herkunftsmarkierung (`Zone.Identifier` ZoneId=3 bzw. `com.apple.quarantine`), versteckte `.part`-Dateien, Aufräumen alter Reste (> 24 h) beim Start                                                                                              | Dateien vom Handy sind fremde Dateien: SmartScreen/Gatekeeper sollen sie wie Browser-Downloads behandeln. Ohne Aufräumen blieben nach Absturz Teil-Dateien in „Downloads“                                |
| 2026-10-24 | Dateinamen vom Handy: Bidi-Steuerzeichen entfernt, max. 200 Byte UTF-8 (Endung bleibt erhalten)                                                                                                                                                                         | RTL-Override täuscht Endungen vor (`gpj.exe`); 255-Byte-Grenzen von NTFS/APFS inklusive `(2)`-Zusatz                                                                                                     |
| 2026-10-24 | Landingpages: Hash-CSP (`sha2`) ohne `unsafe-inline`, `sandbox`-Vorschau, `Host`-Header muss IP oder `localhost` sein                                                                                                                                                   | Inline-Skripte bleiben erlaubt, eingeschleuste nicht; DNS-Rebinding von Webseiten im Browser des Nutzers wird abgewiesen                                                                                 |
| 2026-10-24 | ZIP64 schon ab 2 GiB je Eintrag/Offset                                                                                                                                                                                                                                  | Manche Entpacker lesen 32-Bit-Felder vorzeichenbehaftet; ab 2 GiB scheiterten sie                                                                                                                        |
| 2026-10-24 | Downloads mit schwachem ETag und `If-Range`                                                                                                                                                                                                                             | Fortgesetzte Downloads einer inzwischen geänderten Datei bekommen die ganze Datei statt eines falsch zusammengesetzten Rests                                                                             |
| 2026-10-24 | macOS-Interfaces `en*` werden nicht als WLAN/Ethernet geraten, sondern als „Andere“ eingeordnet                                                                                                                                                                         | `en0` ist je nach Mac WLAN oder Ethernet; eine falsche Beschriftung im Netzwerk-Umschalter verwirrt mehr als keine                                                                                       |
| 2026-10-24 | ~~Updater prüft nicht, solange die Update-Adresse den Platzhalter `OWNER` enthält;~~ (überholt, siehe GitHub-Adresse unten) der Release-Workflow bricht ab, wenn `OWNER` noch drin ist und ein Signatur-Schlüssel gesetzt ist                                           | Nutzerentscheidung im Audit: keine Anfragen an eine nicht existierende Adresse, aber kein Release mit totem Updater                                                                                      |
| 2026-10-24 | Rechte pro Fenster (`default`, `session`, `dropzone`, `welcome`) mit App-Manifest in `build.rs`; Befehle liefern nur Fehlercodes (`CommandError`)                                                                                                                       | Least Privilege: ein Share-Fenster kann z. B. keine Updates installieren; keine Pfade oder Systemmeldungen im Frontend                                                                                   |
| 2026-10-24 | macOS-Dienste übergeben Texte/Empfangswunsch in einem `mktemp -d`-Ordner (0700)                                                                                                                                                                                         | Feste Dateinamen in `$TMPDIR` waren für andere Prozesse desselben Nutzers vorhersagbar (Symlink-/Race-Angriffe)                                                                                          |
| 2026-10-24 | NSIS: bei Updates (`$UpdateMode = 1`) bleiben Kontextmenü, Verknüpfungen und Firewall-Regel unangetastet; Regel heißt „QR Drop (LAN)“, nur TCP, alte Regel wird ersetzt                                                                                                 | Passive Updates sollen keine UAC-Abfrage und keine kurz fehlenden Menüeinträge auslösen; UDP wird nicht gebraucht                                                                                        |
| 2026-10-24 | Reparatur des Kontextmenü-Eintrags (`reg.exe`) läuft in einem Hintergrund-Thread                                                                                                                                                                                        | Drei `reg.exe`-Aufrufe verzögerten sonst jeden Start                                                                                                                                                     |
| 2026-10-24 | Native Fehlermeldung beim Start (`MessageBoxW` bzw. `osascript`), wenn Tauri nicht starten kann (z. B. WebView2 fehlt)                                                                                                                                                  | Sonst passierte für Nicht-Techniker beim Rechtsklick einfach gar nichts                                                                                                                                  |
| 2026-10-24 | macOS: Texte in `Info.plist` (Dokumenttypen, Berechtigungen) über `de.lproj`/`en.lproj/InfoPlist.strings` lokalisiert                                                                                                                                                   | Gleiche Sprache wie die App, ohne zweite Info.plist                                                                                                                                                      |
| 2026-10-24 | Fenster werden direkt sichtbar an selbst berechneter Position erstellt: Mitte des Arbeitsbereichs des Monitors unter der Maus, gestapelt um 28 px versetzt                                                                                                              | `center()` setzte unsichtbar erstellte, rahmenlose Fenster unter Windows an den Bildschirmrand oder auf den zweiten Monitor                                                                              |
| 2026-10-24 | Prozess beendet sich nicht, solange eine Auswahl gesammelt wird oder eine Sitzung vorbereitet ist (`Batches::is_busy`, `has_sessions`)                                                                                                                                  | Kaltstart mit Datei beendete sich sofort: die 300-ms-Sammelzeit endete, bevor das erste Fenster existierte                                                                                               |
| 2026-10-24 | Befehl `share_text` entfernt; `crate-type = ["rlib"]`, tokio-`macros` nur für Tests                                                                                                                                                                                     | Ungenutzter Code und Build-Artefakte (`staticlib`/`cdylib` braucht nur Mobile)                                                                                                                           |
| 2026-10-24 | WebView2 baut selbst HTTPS-Verbindungen zu Microsoft auf; keine Gegenmaßnahme per Browser-Argumenten                                                                                                                                                                    | Gemessen: `--disable-background-networking` u. a. änderten nichts (weiter 2 Verbindungen); kommt vom System-WebView, nicht von QR Drop                                                                   |
| 2026-10-24 | Barrierefreiheit per Linter: `eslint-plugin-jsx-a11y-x` (Konfiguration `strict` plus `control-has-associated-label` und `no-aria-hidden-on-focusable`), `Button` wird als `<button>` geprüft                                                                            | Das Original `eslint-plugin-jsx-a11y` wird seit 2024 nicht gepflegt und unterstützt ESLint 10 nicht; der Fork von es-tooling hat dieselben Regeln und weniger Abhängigkeiten                             |
| 2026-10-24 | Repository `github.com/peter-stuhlmann/QR-Drop`, Update-Adresse eingetragen; Platzhalter-Sperre (`OWNER`) in App und Release-Workflow entfernt                                                                                                                          | Die Sperre konnte mit echter Adresse nie mehr greifen (toter Code). Updates werden ab jetzt automatisch geprüft                                                                                          |
| 2026-10-24 | MIT-Lizenz (`LICENSE`), Autor/Herausgeber „Peter R. Stuhlmann“ in `package.json`, `Cargo.toml` und `bundle` (`publisher`, `copyright`, `homepage`, `license`); keine Lizenzseite im Installer (`licenseFile`)                                                           | Nutzerentscheidung; eine Lizenzseite wäre ein zusätzlicher Klick für Nicht-Techniker. `publisher` vor dem ersten Release geändert, weil der NSIS-Installer ihn für seine Registry-Schlüssel nutzt        |
| 2026-10-24 | README-Screenshots aus der Demo-Seite (`?demo=1`) in `docs/screenshots/`, hell und dunkel per `<picture>`                                                                                                                                                               | Reproduzierbar ohne echtes Gerät; die Demo-Seite zeigt jeden Zustand mit festen Beispieldaten                                                                                                            |
| 2026-10-24 | GitHub Actions auf Commit-SHAs gepinnt (Version als Kommentar) und auf die aktuellen Hauptversionen gehoben (checkout v7, setup-node v7, pnpm/action-setup v6, tauri-action v1); `persist-credentials: false`; Dependabot hält die Pins aktuell, mit 14 Tagen Wartezeit | Ein verschobener Tag kann keinen fremden Code in Build und Signierung bringen. Die alten Hauptversionen liefen auf Node 20, das GitHub abschaltet; die Breaking Changes betreffen unsere Workflows nicht |
