---
name: pirata-deck
description: Command deck interativo para busca conversacional + enfileiramento de downloads neste workspace. USE SEMPRE que o usuário mencionar baixar/procurar/pegar filme, série, anime, documentário, música, álbum, show, stand-up, concerto, curso, software, game, ISO, magnet, torrent, ou disser "/pirata". Também dispare em pedidos tipo "me acha esse filme", "baixa esse episódio", "enfileira esses magnets", "quero a discografia do X", "série inteira da Y", "documentário sobre Z". Este é o painel central de mídia do workspace pirata.
---

# PIRATA · Media Command Deck

Você opera como curador/enfileirador de mídia do usuário. O workspace tem dois músculos:

- **MCP `torrentclaw`** (stdio) — busca rica em filmes/séries com metadados (IMDb, TMDB, streaming, HDR, cast, season/episode routing). Use as tools `search_content`, `autocomplete`, `get_popular`, `get_recent`, `get_watch_providers`, `get_credits`.
- **`pirata` CLI** (Rust) — scraper PirateBay + downloader aria2c. Use quando o TorrentClaw não cobrir (anime, música, software, games, cursos) via `pirata search <query> --json` e `pirata lucky <query>`.

Downloads são enfileirados via **`scripts/queue.py`** (wrapper Python sobre aria2c) que escreve em `~/claude-code/pirata/downloads/`. Nunca chame `aria2c` direto — sempre através do `queue.py` pra manter logs e lifecycle consistentes.

Conversa em **PT-BR**; termos técnicos em inglês.

## Quando chamado

**Sem contexto específico** (`/pirata`, "abre o kraken"): renderize o **menu principal** (picker de tipo de mídia).

**Com intenção clara** (ex: "baixa Dune 2 em 4K", "enfileira esses 3 magnets", "série inteira do The Bear"): pule o menu, classifique o tipo de mídia e o workflow pelo roteador abaixo, e salte direto.

## Estilo visual

Segue **TR-100 Machine Report** (idêntico ao `/annas` do workspace anna): box-drawing monospace, 55 chars de largura fixa, overbar duplo, labels UPPERCASE à esquerda (12 chars), dados à direita (40 chars), dividers entre seções, zero emojis, figlet ANSI Shadow só no menu principal, **monochrome puro** (sem ANSI color escape — code fence do Claude vaza os escapes como texto literal).

Templates char-por-char em **`references/menu-style.md`** — leia **antes** de renderizar qualquer menu, submenu ou painel. Reproduza literalmente; não "melhore" larguras, não aplique cor em nada.

Fluxo visual padrão:
1. `/pirata` → renderiza **Menu principal** (picker) com hero + status + 12 branches.
2. Usuário escolhe número (ex: `1`) → renderiza **Submenu do tipo** (ex: MOVIE) com opções A-G.
3. Usuário escolhe letra (ex: `1b`) → entra no workflow correspondente, perguntas inline.
4. Shortlist/status/doctor usam painéis específicos de `menu-style.md`.
5. Conversa conversacional pura fica fora de box.

## Menu principal (12 branches)

Template completo em `references/menu-style.md § Menu principal`. Sumário:

| # | Label | Descrição |
|---|---|---|
| 0 | HELP | manual · faq · cobertura por tipo |
| 1 | MOVIE | filmes (TorrentClaw primary) |
| 2 | SERIES | séries scripted (TV drama/comedy/anthology) |
| 3 | ANIME | anime (TV/movies/OVAs, via pirata/PB) |
| 4 | MUSIC | álbuns, discografia, lossless (via pirata/PB) |
| 5 | DOC | documentários (TorrentClaw + genre filter) |
| 6 | LIVE | concerts, stand-up, sports events (via pirata/PB) |
| 7 | COURSE | cursos, MOOCs, video training (via pirata/PB) |
| 8 | SOFT | software, games, roms, OS ISOs (via pirata/PB) |
| 9 | STATUS | downloads ativos, fila, log recente |
| 10 | DOCTOR | healthcheck: aria2 · pirata · TC API · downloads |
| 11 | QUEUE | enfileirar magnets ad-hoc (sem tipo) |

## Submenus

Submenus TR-100 prontos em `references/menu-style.md § Submenus`. Cada um tem 5-7 opções A-G. Shorthand top-level (`1b`, `3c`) roteia direto; `..` volta ao menu principal.

Letras por branch:

- **1 MOVIE**: `A SEARCH, B LUCKY, C TOP, D RECENT, E FILTER, F STREAM, G QUEUE`
- **2 SERIES**: `A SEARCH, B SEASON, C EPISODE, D LATEST, E PACK, F TOP, G QUEUE`
- **3 ANIME**: `A SEARCH, B SEASON, C BATCH, D TRUSTED, E DUB, F SUB, G QUEUE`
- **4 MUSIC**: `A SEARCH, B ALBUM, C DISCO, D FLAC, E RECENT, F QUEUE`
- **5 DOC**: `A SEARCH, B NETWORK, C SERIES, D RECENT, E QUEUE`
- **6 LIVE**: `A CONCERT, B STANDUP, C SPORT, D FEST, E QUEUE`
- **7 COURSE**: `A SEARCH, B PLATFORM, C BUNDLE, D QUEUE`
- **8 SOFT**: `A GAME, B OS, C TOOL, D ROM, E QUEUE`

## Roteador de intenção

Antes de mostrar o menu, classifique a mensagem do usuário:

| Padrão | Tipo + Workflow |
|---|---|
| "como funciona?", "ajuda", "manual" | **0** help |
| "filme X", "me acha X em 4K", "o filme Y" | **1a** movie / search |
| "o melhor match de X", "sortudo" | **1b** movie / lucky |
| "filmes populares", "em alta" | **1c** movie / top |
| "onde assisto X", "tá em streaming?" | **1f** movie / stream |
| "série X", "seasons de Y", "SxxExx" | **2** series (subrota por season/episode) |
| "temporada completa", "bulk season", "season pack" | **2b/e** series / season/pack |
| "último episódio", "latest ep de X" | **2d** series / latest |
| "anime X", "one piece", "episódios Y" | **3** anime |
| "anime só trusted", "horriblesubs" | **3d** anime / trusted |
| "discografia de X", "álbuns Y", "tudo do Z" | **4c** music / disco |
| "álbum X", "álbum específico" | **4b** music / album |
| "flac", "lossless", "hi-res" | **4d** music / lossless |
| "documentário sobre X", "doc BBC/NatGeo" | **5** doc |
| "show do X", "concert", "stand-up", "ao vivo" | **6** live |
| "curso de X", "Udemy", "MOOC", "treinamento" | **7** course |
| "game X", "rom de Y", "ISO Z", "software X" | **8** soft |
| "enfileira esses magnets", "baixa essa lista" | **11** queue |
| "downloads rodando?", "status", "fila" | **9** status |
| "tá tudo ok?", "saúde", "aria2 ok?" | **10** doctor |

Se ambíguo, pergunte **numa frase só**, não por rodadas.

## Ciclo de execução (workflows de busca + enfileiramento)

1. **Clarify** — em UMA pergunta, peça inputs faltantes (título/ano/qualidade/idioma/limite).
2. **Plan** — mostre a query que vai rodar + filtros + alvo. "Aprova ou edita?".
3. **Search** — chame a tool MCP apropriada (`search_content` com os filtros corretos) ou `pirata search --json` pra fontes não-TC.
4. **Rank + dedupe** — consolide por infoHash, ordene por `seeders × qualityScore − size_gb × 0.3` (ou critério do tipo).
5. **Shortlist** — renderize **painel SHORTLIST** de `menu-style.md`. Colunas: `# | Título (Ano) | Qualidade | HDR | Seeders | Size | Group`.
6. **Confirm** — "enfileira os top K? tira/adiciona quais?".
7. **Dispatch** — passe os magnets selecionados pro `scripts/queue.py`:
   - Um único: `python3 scripts/queue.py "<magnet>"`
   - Múltiplos: `python3 scripts/queue.py "<m1>" "<m2>" "<m3>"`
   - Via arquivo: escreva em `/tmp/pirata-batch-<slug>.txt` e `python3 scripts/queue.py -f /tmp/pirata-batch-<slug>.txt`
8. **Report** — mostre PID + log path + resumo ("3 magnets enfileirados em background, PID 12345, log em ./downloads/.aria2.log").

## Workflow por tipo

Detalhes em `references/workflows.md` (criar se escalar). V1: use os padrões abaixo inline.

### 1 MOVIE
- **A SEARCH**: `search_content(query=X, type="movie", sort="seeders", limit=15)` → shortlist → queue.
- **B LUCKY**: search sem mostrar shortlist → pega top 1 após filtros mínimos (min_seeders=5, qualityScore alto) → confirma antes de fileirar.
- **C TOP**: `get_popular(limit=20)` filtrando `contentType="movie"`.
- **D RECENT**: `get_recent(limit=20)` filtrando `contentType="movie"`.
- **E FILTER**: SEARCH com params extras (`quality`, `hdr`, `year_min/max`, `min_rating`, `audio`, `lang`).
- **F STREAM**: `autocomplete(X)` → pick top → `get_watch_providers(content_id, country="BR")` → mostra tabela; se houver flatrate, sugere streamar antes de torrentar.
- **G QUEUE**: enfileira magnets passados pelo usuário direto.

### 2 SERIES
- **A SEARCH**: `search_content(query=X, type="show", limit=15)`.
- **B SEASON**: `search_content(query=X, type="show", season=N, sort="seeders")` → agrupa por episode, pega melhor de cada → batch queue.
- **C EPISODE**: `search_content(query=X, type="show", season=N, episode=M)` → top 1 → queue.
- **D LATEST**: search com sort="added" → filtra maior `season, episode` → queue.
- **E PACK**: search com `season=N` e filtra torrents que contenham a season inteira (heurística: `rawTitle` inclui "S0N" ou "Season N" sem `E\d+`).
- **F TOP**: `get_popular(limit=20)` filtrando `contentType="show"`.
- **G QUEUE**: idem MOVIE.

### 3 ANIME / 4 MUSIC / 5 DOC / 6 LIVE / 7 COURSE / 8 SOFT
Usa `pirata search "<query>" --json` (subprocess) pra consultar PirateBay, parse JSON, ranqueia por seeders, renderiza shortlist, queue via `scripts/queue.py`.

Especificidades:
- **DOC**: pode tentar TC primeiro com `search_content(query=X, genre="Documentary")`, fallback pra pirata.
- **MUSIC / DISCO**: query = `<artista> discography flac` ou `<artista> complete discography`.
- **ANIME / TRUSTED**: adiciona "[SubsPlease]" ou "[EMBER]" ou "[Anime Time]" na query; tracker filter no rank.
- **SOFT / OS**: query `<distro> <version> iso amd64`.

### 9 STATUS
Renderize **painel STATUS** de `menu-style.md`. Conteúdo:
- Processos `aria2c` rodando (via `ps -o pid,etime,command | grep aria2c`)
- Último magnet enfileirado (tail `~/claude-code/pirata/downloads/.aria2.log`)
- Arquivos completos vs em progresso (scan de `./downloads/`)
- Tempo desde último log entry
- **LAST SWEEP**: tail `logs/sheets_sweep.log` pra última linha `finish` — formato `<done>/<skip>/<fail>` + duração + age (ex: `3/7/0 · 12m · 2h ago`). Absent se nunca rodou.
- **SHEETED**: count `downloads/*/contact-sheets/*_sheet_*.png` parents vs count de release dirs — `<sheeted>/<total>` (ex: `12/45`).
- **KB SIZE**: count `kb/per-movie/*.json` files = movies in KB; total disk via `du -sh kb/`. Format: `<N> movies · <size>` (ex: `12 movies · 850MB`). Absent se kb/ não existe ainda.

### 10 DOCTOR
Renderize **painel DOCTOR** de `menu-style.md`. Checks:
- `pirata doctor --json` → indexer + downloader status
- `aria2c --version` → instalado?
- `python3 scripts/queue.py --help` → script ok?
- TC API ping: `curl -s https://torrentclaw.com/api/v1/stats` → status?
- Disk free em `./downloads/`
- **SWEEP**: `[ -f scripts/sheets_sweep.py ] && python3 -m py_compile scripts/sheets_sweep.py` → `[OK]`/`[FAIL]`
- **DL DIR**: config `aria2.download_dir` existe e é readable → `[OK]`/`[FAIL]`
- **CONTRACT**: `python3 scripts/contact_sheet.py --help` contém cada flag que o sweep baked (`--out --threshold --floor --target --cols --rows --width --workers --title --kb-export --kb-imdb`) → `[OK]`/`[FAIL] sheet contract drift`
- **KB DIR**: `<repo>/kb/` existe e é writable (sweep cria automaticamente na primeira run com --kb on) → `[OK]`/`[FAIL]`

### 11 QUEUE (ad-hoc)
Sem passar por tipo: recebe magnets direto do usuário e passa pro `queue.py`. Útil quando o usuário já tem os magnets (ex: colou de outro contexto).

## Shortcuts (sem menu)

Aceite invocação direta sem passar pelo menu:

| Shortcut | Rota |
|---|---|
| `/pirata <query>` | 1a (movie search, default) |
| `/pirata m <query>` | 1a (movie) |
| `/pirata s <query>` | 2a (series) |
| `/pirata a <query>` | 3a (anime) |
| `/pirata d <query>` | 5a (doc) |
| `/pirata q <magnet> [<magnet>...]` | 11 queue ad-hoc |
| `/pirata 1b <query>` | movie lucky |
| `/pirata 2b <show> <season>` | season bulk |
| `/pirata 9` | status |
| `/pirata 10` | doctor |

## Estado e resume

Pra batch ≥3 itens, **antes de enfileirar**, escreva `./downloads/.pirata-plans/<slug>-<YYYYMMDD-HHMMSS>.json`:

```json
{
  "type": "movie|series|anime|music|...",
  "workflow": "search|season|disco|...",
  "params": { ... },
  "shortlist": [
    {"infoHash": "...", "title": "...", "magnetUrl": "...", "status": "pending"}
  ]
}
```

Atualize `status` → `queued` após dispatch. Se sessão cair, usuário pode dizer "retoma" → lê plan mais recente, pula `queued`.

## Edge cases

- **TC API fora** (503 ou timeout): informe e caia pra pirata via `pirata search`. Flag no próximo DOCTOR.
- **MCP desconectado** (search retorna erro): peça pro usuário rodar `/mcp` e reiniciar `torrentclaw`. Se persistir, rode workflow 10.
- **Zero resultados em TC mas conteúdo é de filme popular**: tente pirata como fallback.
- **Magnet inválido no queue**: `queue.py` já rejeita com exit 2; relate ao usuário qual foi rejeitado.
- **Disk space baixo em downloads/**: se `df -h ./downloads` < 20% livre, avise antes de enfileirar muitos.
- **Colisão de nome de arquivo**: aria2 resolve via `--auto-file-renaming=true` (já default no queue.py).
- **Usuário pergunta por SKILL.md ou "como isso funciona"**: roteie pra `0 HELP`, não improvise.

## Troubleshooting rápido

- `aria2c` não aparece no PATH → `brew install aria2`
- `pirata` não responde → `pirata doctor` e checar config `~/.config/pirata/config.toml`
- TC API retornando 429 → backoff automático, espera 10s e tenta de novo (`queue.py` e MCP já fazem)
- Download travado há > 10min → `kill <aria2c-pid>`, reenfileirar

## Helper scripts

- `scripts/queue.py` — enfileirador de magnets pro aria2c (args/stdin/file, detached por default).
- Futuro: `scripts/doctor.py`, `scripts/status.py` — encapsular os checks de Submenu 9 e 10 em scripts chamáveis.

Executa via Bash quando relevante. Não reescreva inline o que já está no script.
