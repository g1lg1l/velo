# Piano: AI locale in Velo (nodo AI Image + chat LLM locale)

Branch `feat/local-ai`, base `e900be2` (0.9.6-beta). Documento di lavoro, in italiano:
prima di un PR upstream va tradotto o tolto (CONTRIBUTING chiede un'issue prima di
toccare il decision log, e questo lo tocca: vedi §6).

## 0. Cosa c'è già (e cambia il piano)

**La chat con LLM locale esiste già.** `crates/velo/src/ai.rs`, `Provider::Local`:

- endpoint OpenAI-compatibile (`/v1/chat/completions`), preset Ollama `:11434` e
  LM Studio `:1234`, rilevati da soli mentre girano (`ai::detect()`);
- loop agentico di Velo (`ai::local`, max 24 step) che espone i tool MCP di Velo come
  function calling, con fallback per i modelli piccoli che scrivono le chiamate come
  JSON nel testo (`written_calls`);
- immagini in input se il modello è "vision" (`prefs.sees_images`, euristica `sees()`);
- ogni risposta è un solo passo di undo, e la scena live non si tocca (D19).

Quindi per la chat il lavoro non è "aggiungerla" ma **renderla buona in locale**: §4.

**Manca del tutto la generazione di immagini.** Nessun nodo, nessun provider.
Il piano vero è il nodo **AI Image** (§1–§3).

Pezzi esistenti su cui appoggiarsi:

| Serve | C'è già | Dove |
|---|---|---|
| Mostrare N immagini a turno, una nuova a ogni kick | Media List (fino a 256 file, playback, "new file on every kick") | `velo-core` `MEDIA_LIST`, `velo-engine/src/compile.rs`, `media.rs` |
| Caricare file senza bloccare il render | `media::Library`, un thread per file | `velo-engine/src/media.rs` |
| Frame che arrivano da fuori in continuo | Live node, `Feed` con mailbox lock-free | `velo-engine/src/input.rs` (`ndi:`/`spout:`/`camera:`) |
| Chiamate HTTP a server locali, timeout lunghi, chiavi | `ureq`, `http()`, `key()` nel keychain | `velo/src/ai.rs` |
| Portare i file con il progetto | File → Export with Media impacchetta i path assoluti | `velo/src/files.rs`, `docs/project-format.md §5` |
| Dare alla AI un tool nuovo | lista tool MCP + handler | `velo-mcp/src/lib.rs` `tools()`, `velo/src/agent.rs` `agent_call` |

## 1. Il nodo AI Image: design

**Idea centrale: un AI Image è una Media List i cui file li produce un modello.**
L'engine non sa niente di AI: suona una lista di PNG come fa già. Tutto il lavoro
"lento" (rete, modello, scrittura file) sta nell'app, su un worker thread. Così le
regole di CONTRIBUTING (render loop che non blocca, UI non necessaria per il frame
successivo, il documento come verità) restano vere senza eccezioni.

```
Inspector / chat / MCP ──► imagegen worker (app) ──HTTP──► server locale (sd.cpp, Ollama…)
     prompt, seed                │                              │
                                 ◄──────── PNG (b64) ───────────┘
                                 │ scrive data_dir/generated/<hash>.png (+ .json: prompt, seed, modello)
                                 ▼
             operatore node.add_media (undo, merge)  ──►  documento  ──►  rebuild come Media List
```

**Modello dati** (`velo-core`, nessuna dipendenza nuova):

- `pub const AI_IMAGE: &str = "velo.ai_image";` →
  `kind(AI_IMAGE, "AI Image", Category::Generator, &["ref"], OUT)` (`ref` serve solo in fase 2).
- Riusa i campi esistenti di `Node`: `text` = prompt, `media` = i file generati, uno per
  riga (come Media List). Nessuna migrazione del formato: i file vecchi non hanno il kind,
  e i kind sconosciuti si caricano già (`Node::kind` doc).
- Parametri nell'header: quelli di playback della Media List (ordine, cambio su kick/beat)
  più `pool` (quante immagini tenere, default 8), `steps`, `seed` (-1 = casuale),
  `size` (512 / 768 / 1024), `strength` (img2img, fase 2).
- Operatori nuovi in `ops.rs`:
  - `node.set_text` accetta anche `AI_IMAGE` (estendere `sets_text`): il prompt.
  - `node.add_media {node, path}`: aggiunge una riga e taglia a `pool` (le più vecchie
    escono). Undo, merge per nodo (`run_merge` con chiave `gen <node>`).
  - Funzioni helper: `is_list(kind)` = `MEDIA_LIST || AI_IMAGE`, usata da `media_list()`.

**Engine** (`velo-engine/src/compile.rs`): dove oggi controlla `MEDIA_LIST`
(righe ~91, ~98, ~411) usare `is_list()`. Fine. Un AI Image senza file dà nero,
come una Media List vuota.

**Generare non è un operatore.** Gli operatori sono edit puri del documento; la
generazione è un effetto collaterale lento, come un launch che va dritto all'engine.
Quindi: comando dell'app `generate(node)` → job sul worker → a fine job l'operatore
`node.add_media`. L'undo vede solo l'arrivo del file, mai l'attesa.

**Nuovo modulo `crates/velo/src/imagegen.rs`** (gemello di `ai.rs`):

- `Prefs { endpoint, model, size, steps }` nelle preferenze, sezione "AI and agents";
  preset come `ai::PRESETS`, `detect()` sulle porte note, `check()` per "Test the connection".
- `generate(prefs, prompt, seed, reference: Option<&[u8]>) -> Result<Vec<u8>, String>`:
  `POST {endpoint}/images/generations` con `{model, prompt, n, size, response_format: "b64_json"}`;
  img2img via `POST {endpoint}/images/edits` (multipart scritto a mano: ureq 3 non lo ha,
  sono ~30 righe, nessuna dipendenza nuova).
- Una coda per processo, **un job alla volta** (la memoria unificata del Mac non regge due
  diffusioni in parallelo accanto a un LLM), annullabile, con progresso a eventi come
  `ai::Event`.
- File in `files::data_dir()/generated/<sha256 del png>.png` + sidecar `.json`
  (prompt, seed, modello, passi): riproducibile, deduplicato, e già esportabile.

**UI** (`crates/velo/src/main.rs`, inspector, accanto alla sezione FILE ~riga 2452):
campo prompt multilinea, bottone **Generate** (e "×N"), stato/progresso, striscia di
miniature del pool (click = rimuovi), seed bloccabile. Solo token del tema, niente
literal di colore (CI). Testo via `tr!` + `i18n/it.rs`.

**Scena live.** Stessa regola della AI di oggi: un AI Image dentro la scena live non
riceve file nuovi se le preferenze non lo permettono; altrimenti i file aspettano il
prossimo launch della scena. Motivo: `media` entra nell'hash strutturale
(`live.rs` ~riga 944), quindi ogni file nuovo è un rebuild. Il rebuild avviene al bordo
del frame, ma va misurato se la Media List riparte da capo (gate G2).

## 2. Backend per le immagini: un solo protocollo

Stessa scelta di D19 per la chat: **parlare un protocollo, non un prodotto.** L'API
OpenAI `images/generations` (+ `images/edits`) è già esposta da più runtime locali:

| Runtime | Note | Ruolo |
|---|---|---|
| **stable-diffusion.cpp** `sd-server` | C/C++, Metal, binario singolo, MIT; SD1.5/SDXL/Turbo/FLUX/Z-Image; espone `/v1/images/generations` e `/v1/images/edits` | **Riferimento** per sviluppo e test |
| **Ollama** (sperimentale, macOS, da gen 2026) | `x/z-image-turbo`, FLUX.2 klein 4B/9B; endpoint OpenAI per le immagini solo su un branch di sviluppo, **da verificare** sulla versione installata | Preset se l'endpoint c'è: stesso server della chat |
| **LocalAI**, OpenVINO Model Server | Stesso schema OpenAI | Funzionano gratis come endpoint custom |
| **ComfyUI** | API propria (`/prompt`, `/history`, websocket), workflow JSON | Adapter separato, fase 4, solo se richiesto |
| Draw Things, mflux | Ottimi su Mac ma API diverse / solo CLI | Fuori scope |

**Modelli suggeriti** (da ricontrollare al momento, cambiano in fretta):
Z-Image-Turbo (6B, ~8 passi) come default qualità/velocità; FLUX.2 klein 4B;
SD-Turbo / SDXL-Turbo per iterare a 512 in 1–4 passi.
**Attenzione licenze dei pesi:** i VJ fanno serate pagate. Alcuni pesi sono
non-commerciali (es. FLUX.2 klein 9B secondo fonti secondarie, la famiglia SDXL-Turbo
sotto licenza community di Stability). Il pannello deve mostrare la licenza del modello
scelto, o almeno un avviso; Velo non distribuisce pesi.

## 3. Fasi

Ogni fase è un PR piccolo (CONTRIBUTING: "one concern each"), con test.

**F0 — Issue e misure (prima di scrivere codice)**
- Issue upstream con questo design (D21 sotto). Il repo è di g1lg1l: per i PR serve un fork.
- Misure su Mac 64GB, una tabella: secondi per immagine 512/768/1024 con sd.cpp e Ollama
  (Z-Image-Turbo, klein 4B, SD-Turbo); RAM con LLM della chat caricato in parallelo.
- *Done:* numeri reali, scelta del default.

**F1 — AI Image MVP (txt2img, manuale)**
- `velo-core`: kind, `is_list`, `node.add_media`, `sets_text`; test in `velo-core/src/tests.rs`
  (aggiunta, taglio a `pool`, undo, round-trip RON, kind nel catalogo).
- `velo-engine`: `compile.rs` via `is_list`; test headless: un AI Image con 2 PNG di fixture
  rende come una Media List.
- `velo`: `imagegen.rs`, preferenze, inspector, `tr!`/it; test unitari su request/response
  (JSON di fixture, nessuna rete) come quelli di `ai.rs`.
- Icona, voce nel node menu, README ("64 nodes" → 65), `docs/graph.md`.
- *Done:* Tab → AI Image → prompt → Generate → l'immagine gira sull'Output e cambia sul kick.

**F2 — La chat usa il nodo**
- Tool MCP `generate_image {node?, prompt, n}` in `velo-mcp::tools()` + `agent.rs`.
  **Non deve bloccare:** `agent_call` gira sul thread UI (`poll_agent`), quindi il tool
  accoda il job e risponde subito con un id; `generation_status {id}` per sapere quando è
  pronto. Il loop locale aspetta chiamando lo status.
- BRIEF: quando una scena chiede un'immagine figurativa (un logo, un soggetto), creare un
  AI Image invece di forzarla in uno shader. L'LLM scrive il prompt per il modello immagini.
- Funziona uguale con Claude Code / Codex via `velo-mcp`: stesso tool.
- *Done:* "fammi una scena con meduse al neon che pulsano sul kick" dal pannello, tutto locale.

**F3 — img2img e trigger musicali**
- Input `ref`: serve leggere dalla GPU l'uscita di un nodo. Oggi c'è `Command::Peek` per
  la scena (snapshot); va esteso a un nodo, consegnato in RGBA come lo snapshot.
  MVP alternativo: `ref` da file o dallo snapshot della scena intera.
- "Rigenera su launch / ogni N bar / sul drop": il trigger parte dall'app leggendo lo
  `Status` (beat, drop), mai dal render thread. Mai più di un job in coda per nodo.

**F4 — Sperimentale: sorgente live `ai:`**
- In `input.rs`, kind `ai:<preset>`: un thread che fa img2img in loop sull'ultimo frame
  a 512 con un modello a 1 passo e mette il risultato nel `Feed`. Il Live node fa il resto.
- **Gate G3 prima di iniziare:** su Mac aspettarsi pochi fps (StreamDiffusion e simili
  sono pensati per CUDA). Se la misura F0 dice < 2 fps a 512, non costruirlo: il pool
  pre-generato di F1/F3 copre l'uso live meglio.
- ComfyUI come secondo adapter solo qui, se serve.

## 4. Chat LLM locale: cosa migliorare

Tutto in `crates/velo/src/ai.rs` / `chat.rs`, niente di strutturale.

1. **Preset mancanti**: `llama-server` di llama.cpp e `mlx_lm.server` (entrambi `:8080` di
   default, quindi un solo preset "llama.cpp / MLX"), Jan, LocalAI. Anche il tuo
   `ds4-server` come endpoint custom, se espone `/v1/chat/completions` con i tool: da verificare.
2. **Streaming**: oggi `"stream": false` con timeout di 600 s: in locale si guarda una
   rotella per minuti. SSE con delta di testo e di `tool_calls`.
3. **Budget di contesto**: system prompt (BRIEF + `velo_mcp::INSTRUCTIONS`) + 12 schemi
   di tool + l'output di `list_nodes` (65 kind con parametri) possono superare il contesto
   di default di Ollama, che **tronca in silenzio**. Misurare i token in F0; poi stimarli
   prima dell'invio, leggere il contesto vero (`/api/show` su Ollama) e avvisare
   (`OLLAMA_CONTEXT_LENGTH`), più un set di tool ridotto per modelli piccoli.
4. **Affidabilità dei tool**: un piccolo eval ripetibile, 10 prompt, punteggio da
   `query_project` (la scena arriva all'Output? ha almeno una mappatura? lo shader compila?).
   Serve per dire quali modelli locali consigliare invece di indovinarlo.
5. **Memoria condivisa**: LLM + modello immagini + Velo sulla stessa RAM unificata.
   `keep_alive` corto per il modello immagini su Ollama, e un chip di stato nel pannello.

## 5. Rischi e gate

| # | Rischio | Gate / mitigazione |
|---|---|---|
| G1 | Generazione troppo lenta per l'uso live | Misura F0; il design lavora a pool pre-generato, mai per frame |
| G2 | Un file nuovo in una scena live fa ripartire o scattare la Media List | Test in F1; fallback: file nuovi applicati al prossimo launch |
| G3 | Realtime `ai:` irraggiungibile su Mac | Non iniziare F4 sotto 2 fps misurati |
| G4 | Contesto troncato in silenzio sui modelli locali | §4.3 |
| G5 | Licenze dei pesi in serate pagate | Avviso nel pannello, nessun peso distribuito |
| G6 | Memoria: LLM + diffusione + GPU di Velo | Un job alla volta, scaricamento del modello, misura in F0 |

## 6. Decisione da proporre upstream

> **D21** — Le immagini generate entrano come file: un nodo AI Image è una Media List
> i cui file li scrive un modello dietro un endpoint OpenAI-compatibile per le immagini
> (`images/generations`, `images/edits`), chiamato da un worker dell'app. L'engine non
> fa rete né aspetta un modello. *Alternative:* diffusione dentro l'engine (wgpu/candle),
> un nodo che parla a ComfyUI, una sorgente live realtime. *Perché:* il render thread
> non aspetta mai, i file vanno nell'export e nell'undo come ogni media, un protocollo
> copre più runtime locali (stessa logica di D19). *Rivedere quando:* un modello locale
> regge img2img a frame rate su un portatile.

Nessuna dipendenza nuova: `ureq`, `image`, `serde_json`, `keyring` ci sono già.
Licenze: solo chiamate HTTP verso processi esterni, niente da linkare.
