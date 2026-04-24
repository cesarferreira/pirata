# Menu Style — TR-100 aesthetic · PIRATA deck

Contrato visual pro output de menu/painel da skill `/pirata`. Idêntico ao `/annas` (TR-100 Machine Report da U.S. Graphics Co): box-drawing monospace, **monochrome puro**, figlet hero ANSI Shadow, zero emojis, zero ANSI color escapes.

**Sempre renderize dentro de um code fence de 3 backticks** (sem language tag) pra preservar whitespace e monospace. Reproduza os templates char-a-char — não "melhore" larguras, não adicione colorização, não edite o figlet. Code fence do Claude não interpreta escape codes (`\x1b[...]m` vaza como texto literal), então monochrome é obrigatório.

## Princípio de design

Uma frame, uma grid, três registros visuais:

1. **Hero** (só menu principal): figlet PIRATA + subtitle, full-width
2. **Status** (grid 2-col): dados do sistema (indexer, downloads dir, fila ativa)
3. **Menu** (grid 2-col): 12 branches numeradas (content type picker)

Submenus e painéis usam só os registros 2+3 + título centrado — sem hero.

## Grid fixo

Todos templates: **55 chars de largura**.

- Label col: 12 chars
- Data col: 40 chars
- Column split na posição 14

Verificação: `│` + 12 + `│` + 40 + `│` = 1+12+1+40+1 = **55 chars**.

## Primitivas de caixa

```
┌   ┐   └   ┘      corners
├   ┤   ┬   ┴   ┼  T-junctions and cross
─   │              horizontal, vertical
```

Double-line chars (`╔╗╚╝║═╠╣╦╩╬`) **só aparecem dentro do figlet ANSI Shadow** (usa `╗╚╝═` naturalmente). Fora do figlet: single-line only.

## Header "overbar"

Primeira linha `┌──..──┐` + segunda linha `├──..──┤` antes da área de conteúdo.

```
┌─────────────────────────────────────────────────────┐
├─────────────────────────────────────────────────────┤
```

## Hero row format (só menu principal)

Full-width, 53 chars internos entre `│`s.

- **Empty spacer**: `│` + 53 spaces + `│`
- **Figlet row**: `│` + 4 spaces + 44 figlet chars + 5 spaces + `│`
- **Subtitle**: centralizado (padding igual nos dois lados)

## Column split divider

Transição full-width → 2-col:

```
├────────────┬────────────────────────────────────────┤
```

## Middle divider (dentro 2-col)

```
├────────────┼────────────────────────────────────────┤
```

## Bottom

```
└────────────┴────────────────────────────────────────┘
```

## Label col (12 chars)

### Main menu (numéric)
- 1-digit: `  X  LABEL   ` (2 lead + digit + 2 sep + label + trail pad)
- 2-digit: ` XX  LABEL   ` (1 lead + 2 digits + 2 sep + label + trail pad)

### Submenus (letra)
```
  A  SEARCH 
  B  LUCKY  
  C  TOP    
```
Formato: 2 lead + letra + 2 sep + label + trail pad.

### Panels (label-only)
```
 INDEXER    
 DOWNLOADS  
 QUEUE      
```
Formato: 1 lead + LABEL + trail pad.

## Data col (40 chars)

Left-aligned. Descrições curtas (≤35 chars preferível).

## Status badges

Direita-alinhados entre `[...]`:

- `[LIVE]` / `[DOWN]`
- `[CONNECTED]` / `[DISCONNECTED]`
- `[HTTP200]` / `[HTTP429]` / `[HTTP000]`
- `[OK]` / `[WARN]` / `[FAIL]`
- `[ACTIVE]` / `[IDLE]` / `[QUEUED]`
- `[RUNNING]` / `[DONE]` / `[STALLED]`

## Bar graph

36 blocks em 40-col data. `█` filled, `░` empty.

```
│ DISK USE   │ ████████████████████████████████████░░ │
```

## Policy de cor

Monochrome estrito em todo output. Nenhum ANSI escape code (`\x1b[...]m`) — code fence do Claude não interpreta e vaza como texto literal. Bars em painéis usam só `█░`.

Zero emojis. Zero shadeblocks como decoração (só em bar graphs). Zero tracking (não espaçar letras tipo "P I R A T A"). Zero cor.

## Tom

Labels em **inglês técnico UPPERCASE** (MOVIE, SERIES, INDEXER, SEARCH, LUCKY). Descriptions em **português curto** com termos técnicos em inglês. Exemplo: `│  1  MOVIE  │ filmes · scripted · indie              │`.

## Input footer convention

Última row antes do bottom. Label col vazio, data col mostra formato + back hint:

```
├────────────┼────────────────────────────────────────┤
│            │ number · letter · free text · .. back  │
└────────────┴────────────────────────────────────────┘
```

Main menu: sem `.. back` (já é o top); sair = `exit`/`..`.
Submenus: `.. back` volta ao main menu.

---

# Templates

Reproduza char-a-char. Monochrome puro, sem exceção.

## Menu principal (picker de tipo de mídia)

```
┌─────────────────────────────────────────────────────┐
├─────────────────────────────────────────────────────┤
│                                                     │
│    ██████╗ ██╗██████╗  █████╗ ████████╗ █████╗      │
│    ██╔══██╗██║██╔══██╗██╔══██╗╚══██╔══╝██╔══██╗     │
│    ██████╔╝██║██████╔╝███████║   ██║   ███████║     │
│    ██╔═══╝ ██║██╔══██╗██╔══██║   ██║   ██╔══██║     │
│    ██║     ██║██║  ██║██║  ██║   ██║   ██║  ██║     │
│    ╚═╝     ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝   ╚═╝   ╚═╝  ╚═╝     │
│                                                     │
│       MEDIA · COMMAND DECK · DANTE WORKSPACE        │
│                                                     │
├────────────┬────────────────────────────────────────┤
│ INDEXER    │ torrentclaw.com                [LIVE]  │
│ DOWNLOADS  │ ~/claude-code/pirata/downloads         │
│ QUEUE      │ 0 active · 0 completed today           │
├────────────┴────────────────────────────────────────┤
│                  SELECT MEDIA TYPE                  │
├────────────┬────────────────────────────────────────┤
│  0  HELP   │ manual · faq · coverage by type        │
│  1  MOVIE  │ filmes · scripted · indie              │
│  2  SERIES │ séries · drama · comedy · anthology    │
│  3  ANIME  │ anime · tv · movies · ovas             │
│  4  MUSIC  │ álbuns · discografia · lossless        │
│  5  DOC    │ documentários · cinema · tv · nature   │
│  6  LIVE   │ concerts · stand-up · sports · fest    │
│  7  COURSE │ cursos · mooc · video training         │
│  8  SOFT   │ software · games · roms · os iso       │
│  9  STATUS │ downloads ativos · fila · log recente  │
│ 10  DOCTOR │ health: aria2 · pirata · tc · disk     │
│ 11  QUEUE  │ enfileirar magnets ad-hoc              │
├────────────┼────────────────────────────────────────┤
│            │ number · free text · exit              │
└────────────┴────────────────────────────────────────┘
```

**Valores dinâmicos a substituir ao renderizar:**
- `INDEXER` — `torrentclaw.com` com badge `[LIVE]` se `curl -s https://torrentclaw.com/api/v1/stats` retornar 2xx; `[DOWN]` caso contrário.
- `DOWNLOADS` — path do config do pirata (encurtar com `~/`).
- `QUEUE` — `<N> active · <M> completed today`. N = processos aria2c rodando; M = arquivos com mtime dentro das últimas 24h em `./downloads/`.

**Integridade:**
- 55 chars por linha, zero exceção.
- Figlet ANSI Shadow é sagrado — nunca edite.
- SELECT MEDIA TYPE banner é full-width (sem column split). A grid re-engaja depois no `├──┼──┤` seguinte.

**Linha do SELECT MEDIA TYPE banner:**

A linha do banner "SELECT MEDIA TYPE" é full-width (`├────────────┼────────────────────────────────────────┤` antes e depois). Na prática, o char na posição 14 (col split) é `┼` nas divisórias adjacentes mas o banner interno ignora o split — apenas 53 chars de texto centralizado. Observação: a linha acima mostra `│                 SELECT MEDIA TYPE                   │` com 53 chars internos centralizados ("SELECT MEDIA TYPE" = 17 chars, padding 18/18).

## Submenus

Pattern: overbar + título centrado (full-width) + column split + opções A-G + middle divider + input footer.

### `0 HELP`

```
┌─────────────────────────────────────────────────────┐
├─────────────────────────────────────────────────────┤
│                      HELP / FAQ                     │
├────────────┬────────────────────────────────────────┤
│  A  TOUR   │ visão geral do pirata deck             │
│  B  TYPES  │ cobertura por tipo de mídia            │
│  C  MCP    │ torrentclaw tools · params · limits    │
│  D  PB     │ pirata CLI · piratebay scraper         │
│  E  QUEUE  │ aria2c via scripts/queue.py            │
│  F  TROUBLE│ erros comuns · fixes                   │
│  G  FAQ    │ free-form questions                    │
├────────────┼────────────────────────────────────────┤
│            │ letter · free text · .. to back        │
└────────────┴────────────────────────────────────────┘
```

### `1 MOVIE`

```
┌─────────────────────────────────────────────────────┐
├─────────────────────────────────────────────────────┤
│                       MOVIES                        │
├────────────┬────────────────────────────────────────┤
│ TYPE       │ movie                        [ACTIVE]  │
│ INDEXER    │ torrentclaw (metadata-rich)            │
├────────────┼────────────────────────────────────────┤
│  A  SEARCH │ busca por título                       │
│  B  LUCKY  │ best-match automático (one-shot)       │
│  C  TOP    │ populares agora                        │
│  D  RECENT │ recém-adicionados                      │
│  E  FILTER │ quality · hdr · year · rating · audio  │
│  F  STREAM │ onde assistir (Netflix/Disney+ · BR)   │
│  G  QUEUE  │ enfileira magnets direto               │
├────────────┼────────────────────────────────────────┤
│            │ letter · free text · .. to back        │
└────────────┴────────────────────────────────────────┘
```

### `2 SERIES`

```
┌─────────────────────────────────────────────────────┐
├─────────────────────────────────────────────────────┤
│                       SERIES                        │
├────────────┬────────────────────────────────────────┤
│ TYPE       │ show                         [ACTIVE]  │
│ INDEXER    │ torrentclaw (season/episode routing)   │
├────────────┼────────────────────────────────────────┤
│  A  SEARCH │ busca por título                       │
│  B  SEASON │ temporada completa (bulk episodes)     │
│  C  EPISODE│ episódio único (SxxExx)                │
│  D  LATEST │ último episódio disponível             │
│  E  PACK   │ season pack (todos num torrent)        │
│  F  TOP    │ séries populares                       │
│  G  QUEUE  │ enfileira magnets direto               │
├────────────┼────────────────────────────────────────┤
│            │ letter · free text · .. to back        │
└────────────┴────────────────────────────────────────┘
```

### `3 ANIME`

```
┌─────────────────────────────────────────────────────┐
├─────────────────────────────────────────────────────┤
│                        ANIME                        │
├────────────┬────────────────────────────────────────┤
│ TYPE       │ anime                        [ACTIVE]  │
│ INDEXER    │ piratebay via pirata search --json     │
├────────────┼────────────────────────────────────────┤
│  A  SEARCH │ busca por título                       │
│  B  SEASON │ temporada (range de episódios)         │
│  C  BATCH  │ batch BD release (season pack)         │
│  D  TRUSTED│ apenas trusted groups (SubsPlease...)  │
│  E  DUB    │ apenas dublado                         │
│  F  SUB    │ apenas legendado                       │
│  G  QUEUE  │ enfileira magnets direto               │
├────────────┼────────────────────────────────────────┤
│            │ letter · free text · .. to back        │
└────────────┴────────────────────────────────────────┘
```

### `4 MUSIC`

```
┌─────────────────────────────────────────────────────┐
├─────────────────────────────────────────────────────┤
│                        MUSIC                        │
├────────────┬────────────────────────────────────────┤
│ TYPE       │ music                        [ACTIVE]  │
│ INDEXER    │ piratebay via pirata search --json     │
├────────────┼────────────────────────────────────────┤
│  A  SEARCH │ busca por artista/álbum                │
│  B  ALBUM  │ álbum específico                       │
│  C  DISCO  │ discografia completa                   │
│  D  FLAC   │ lossless · FLAC · WAV · hi-res         │
│  E  RECENT │ lançamentos novos                      │
│  F  QUEUE  │ enfileira magnets direto               │
├────────────┼────────────────────────────────────────┤
│            │ letter · free text · .. to back        │
└────────────┴────────────────────────────────────────┘
```

### `5 DOC`

```
┌─────────────────────────────────────────────────────┐
├─────────────────────────────────────────────────────┤
│                   DOCUMENTARIES                     │
├────────────┬────────────────────────────────────────┤
│ TYPE       │ doc                          [ACTIVE]  │
│ INDEXER    │ torrentclaw (genre=Documentary)        │
├────────────┼────────────────────────────────────────┤
│  A  SEARCH │ busca por tema                         │
│  B  NETWORK│ BBC · Nat Geo · ZDF · Arte · HBO       │
│  C  SERIES │ doc series (Our Planet, Chef's Table)  │
│  D  RECENT │ recém-lançados                         │
│  E  QUEUE  │ enfileira magnets direto               │
├────────────┼────────────────────────────────────────┤
│            │ letter · free text · .. to back        │
└────────────┴────────────────────────────────────────┘
```

### `6 LIVE`

```
┌─────────────────────────────────────────────────────┐
├─────────────────────────────────────────────────────┤
│                    LIVE EVENTS                      │
├────────────┬────────────────────────────────────────┤
│ TYPE       │ live                         [ACTIVE]  │
│ INDEXER    │ piratebay via pirata search --json     │
├────────────┼────────────────────────────────────────┤
│  A  CONCERT│ shows musicais · festivais             │
│  B  STANDUP│ stand-up comedy specials               │
│  C  SPORT  │ eventos esportivos (UFC · F1 · futebol)│
│  D  FEST   │ festivais (Lollapalooza, Coachella...) │
│  E  QUEUE  │ enfileira magnets direto               │
├────────────┼────────────────────────────────────────┤
│            │ letter · free text · .. to back        │
└────────────┴────────────────────────────────────────┘
```

### `7 COURSE`

```
┌─────────────────────────────────────────────────────┐
├─────────────────────────────────────────────────────┤
│                      COURSES                        │
├────────────┬────────────────────────────────────────┤
│ TYPE       │ course                       [ACTIVE]  │
│ INDEXER    │ piratebay via pirata search --json     │
├────────────┼────────────────────────────────────────┤
│  A  SEARCH │ busca por tema/skill                   │
│  B  PLATFOR│ por plataforma (Udemy · Pluralsight)   │
│  C  BUNDLE │ bundle/collection                      │
│  D  QUEUE  │ enfileira magnets direto               │
├────────────┼────────────────────────────────────────┤
│            │ letter · free text · .. to back        │
└────────────┴────────────────────────────────────────┘
```

### `8 SOFT`

```
┌─────────────────────────────────────────────────────┐
├─────────────────────────────────────────────────────┤
│                  SOFTWARE & GAMES                   │
├────────────┬────────────────────────────────────────┤
│ TYPE       │ soft                         [ACTIVE]  │
│ INDEXER    │ piratebay via pirata search --json     │
├────────────┼────────────────────────────────────────┤
│  A  GAME   │ games (PC · console)                   │
│  B  OS     │ Linux · Windows · macOS ISOs           │
│  C  TOOL   │ ferramentas · apps · utilitários       │
│  D  ROM    │ roms retrô (NES · SNES · N64 · PS1)    │
│  E  QUEUE  │ enfileira magnets direto               │
├────────────┼────────────────────────────────────────┤
│            │ letter · free text · .. to back        │
└────────────┴────────────────────────────────────────┘
```

## Painéis de dados

### STATUS (downloads ativos)

```
┌─────────────────────────────────────────────────────┐
├─────────────────────────────────────────────────────┤
│                  STATUS · PIRATA                    │
├────────────┬────────────────────────────────────────┤
│ ACTIVE     │ 3 processes                            │
│ QUEUE SIZE │ 12 magnets pending                     │
│ DONE TODAY │ 5 files (2026-04-24)                   │
├────────────┼────────────────────────────────────────┤
│ PID 12345  │ ubuntu-24.04.iso              [75.2%]  │
│ PID 12346  │ dune-part-two-2160p           [22.8%]  │
│ PID 12347  │ the-bear-s03                   [8.1%]  │
├────────────┼────────────────────────────────────────┤
│ LOG        │ ./downloads/.aria2.log (15s ago)       │
│ DISK FREE  │ 412 GiB / 1.8 TiB             [22.9%]  │
│ DISK USE   │ ██████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░ │
├────────────┼────────────────────────────────────────┤
│ LAST SWEEP │ 3/7/0 · 12m · 2h ago                   │
│ SHEETED    │ 12/45 releases                         │
│ KB SIZE    │ 12 movies · 850MB                      │
├────────────┼────────────────────────────────────────┤
│ ADVICE     │ 3 downloads rodando ok. tail log pra ver│
└────────────┴────────────────────────────────────────┘
```

### DOCTOR (healthcheck)

```
┌─────────────────────────────────────────────────────┐
├─────────────────────────────────────────────────────┤
│                  DOCTOR · PIRATA                    │
├────────────┬────────────────────────────────────────┤
│ ARIA2C     │ /opt/homebrew/bin/aria2c         [OK]  │
│ PIRATA     │ 0.1.0 · aria2 downloader         [OK]  │
│ QUEUE.PY   │ scripts/queue.py                 [OK]  │
│ MCP TC     │ torrentclaw              [CONNECTED]   │
│ TC API     │ torrentclaw.com             [HTTP200]  │
├────────────┼────────────────────────────────────────┤
│ DOWNLOADS  │ ~/claude-code/pirata/downloads         │
│ DISK FREE  │ 412 GiB / 1.8 TiB             [22.9%]  │
│ DISK USE   │ ██████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░ │
├────────────┼────────────────────────────────────────┤
│ SWEEP      │ scripts/sheets_sweep.py          [OK]  │
│ DL DIR     │ ~/claude-code/pirata/downloads   [OK]  │
│ CONTRACT   │ contact_sheet.py flags intact    [OK]  │
│ KB DIR     │ ~/claude-code/pirata/kb/         [OK]  │
├────────────┼────────────────────────────────────────┤
│ ADVICE     │ pipeline ok. tudo pronto pra fila.     │
└────────────┴────────────────────────────────────────┘
```

### SHORTLIST (após search + rank)

```
┌─────────────────────────────────────────────────────┐
├─────────────────────────────────────────────────────┤
│              SHORTLIST · MOVIE · DUNE               │
├────────────┬────────────────────────────────────────┤
│ TOTAL HITS │ 47 (deduped: 31)                       │
│ SHOWING    │ top 5 · sort=seeders                   │
├────────────┼────────────────────────────────────────┤
│ 1 2160p HDR│ Dune.2021.UHD.BluRay · [2840]  · 42GB  │
│ 2 1080p    │ Dune.2021.BluRay.x265 · [1921]  ·  8GB │
│ 3 2160p    │ Dune.2021.WEB-DL.4K   · [1402]  · 18GB │
│ 4 1080p    │ Dune.2021.AMZN.WEB    · [ 987]  ·  5GB │
│ 5 720p     │ Dune.2021.WEBRip      · [ 634]  ·  2GB │
├────────────┼────────────────────────────────────────┤
│            │ queue · queue N · skip N · .. back     │
└────────────┴────────────────────────────────────────┘
```

## Quando escapar do template

- Resposta conversacional pura → sem box.
- Mensagem de erro curta → inline.
- Confirmação "enfileirei N em ./downloads/" → 1 linha, sem box.

Boxes são pra **decisões** (menus, submenus, shortlist) e **status estruturado** (doctor, status). Nunca pra decorar output casual.

## Lembrete final

- Se tiver dúvida se box cabe → prefira inline.
- Figlet hero = só no menu principal, nunca colorido.
- Resto é monochrome disciplinado.
