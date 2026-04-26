# Memory Deep #002

| Field       | Value                                              |
|-------------|----------------------------------------------------|
| Created     | 2026-04-24 20:48 BRT                               |
| Project     | pirata — personal media download + contact-sheet workspace |
| Session     | First real KB export run on Roger Rabbit; refactored kb sheet from "clean" to "labeled-jpeg-lighter" per user feedback; renamed kb dir; one commit. |
| Previous    | MEMORY_DEEP_001.md                                |

---

## Project Context

`pirata` is Vidigal's personal Mac-based media workspace at `~/claude-code/pirata`. Two muscles: (a) a `torrentclaw` MCP for rich movie/TV search with metadata, and (b) a Rust `pirata` CLI scraper for non-TC sources. Downloads go through `aria2c` orchestrated by `scripts/queue.py`. On top: cinema-grade contact-sheet pipeline for human review (release/contact-sheets/) and a parallel KB export for RAG-multimodal ingest (kb/). Path-agnostic sweeper picks up any new release dir without sheets.

## What Happened This Session

Short session, three discrete acts:

### Act 1: First real-world KB export validation (Roger Rabbit)

Loaded snapshot 001, status report, then user requested first end-to-end KB test on Roger Rabbit. State at start: kb/ inexistente, disco em 91% usado (9% livre — abaixo do gate de 10% do sweep), Roger Rabbit em downloads/ com sheets antigas em `contact/` (pré-pivô do sweeper) e `contact-sheets/` vazio.

**Decision: bypassar o sweep e invocar `contact_sheet.py` direto.** Razões:
- Sweep gate é `<10% livre → SKIP` (DISK_FREE_FLOOR=0.10 em sheets_sweep.py:49). Disco em 9% bloqueia.
- `contact_sheet.py` direto não tem gate.
- `contact/` já tem `scenes_raw_t8.txt` (cache do scdet, 11 KB) — apontar `--out` pra esse dir reusa o cache (poupa ~9 min).
- mkv intacto: 5.07 GB.

**Run 1 (v1):**
```bash
python3 -u scripts/contact_sheet.py \
  "downloads/Who Framed Roger Rabbit (1988) ...]/Who.Framed.Roger.Rabbit...mkv" \
  --out "downloads/Who Framed Roger Rabbit ...]/contact" \
  --kb-export kb \
  --title "Who Framed Roger Rabbit (1988)"
```

Tempo total ~135s. Cache hit no scdet (1331 cenas instantâneas), floor 4s → 625, cap → 300. Extract 300 frames em 133s. Tile 3 sheets PNG (porque defaults `cols=10 rows=10` = 100 thumbs/sheet, contra layout original 6×5 com 10 sheets). KB export ok: frames JPEG (7.8 MB) + clean sheets JPEG (7.2 MB) + per-movie JSON (68 KB) + manifest.jsonl (72 KB, 300 linhas).

Heads-up que dei ao usuário: layout cosmeticamente diferente do original (3 sheets de 100 vs 10 sheets de 30), `contact/` ficou com mix de sheets antigas (4-10 órfãs, 6×5) + novas (1-3 com 10×10).

### Act 2: User feedback → refactor "clean sheet" → "labeled jpeg sheet"

Usuário confirmou que o pipeline tava no jeito (prepara, não ingesta). Depois levantou: **"o contact-sheet para kb, acho importante ter....a numeracao e o timecode...como no contact sheet original...ele tem que ser so uma versao mais leve do original"**.

Isso reverteu a decisão original do snapshot 001 ("Clean sheets: Same 10×(6×5) as labeled, minus captions+header"). Nova diretriz: KB sheet preserva captions + header, só vira JPEG q=90.

**Refactor implementado (single commit):**

1. `scripts/contact_sheet.py`:
   - `tile_sheets()` simplificado: removido param `clean=False`, removida branch que aceitava `Path` em `items`, removido `tag = "tile-clean" if clean else "tile"`. Agora aceita só (idx, t, labeled_image) e sempre desenha header + sempre usa as Images já compostas.
   - `export_kb()`: assinatura ganha `labeled: list` e `header_font_size: int`. Em vez de re-tilear raw paths com clean=True, agora chama `tile_sheets(labeled, ..., ext="jpg")` — mesmo header + caption strip do PNG release, só JPEG.
   - Dir renomeado: `kb/contact-sheets-clean/` → `kb/contact-sheets/`. O nome "clean" virou mentira.
   - Variable rename interno: `clean_sheets` → `kb_sheets`.
   - Comment update: "Re-tile clean sheets from raw paths (no caption strip, no header)" → "Re-tile labeled sheets as JPEG — same numbering + TC + header as the release PNG sheet, just lighter (~30% size)".
   - `main()` atualizado pra passar `labeled` + `header_font_size`.

2. `scripts/tests/test_kb_export.sh`:
   - `SHEETS_DIR` path: `kb/contact-sheets-clean/$SLUG` → `kb/contact-sheets/$SLUG`.
   - T1b: `"T1b clean sheets dir"` → `"T1b kb sheets dir"`.
   - T8: assertion completamente reescrita. Era: "clean sheet has no header band (height smaller than labeled by ≥40px)". Virou: "kb sheet matches labeled dimensions (header preserved)". Tentei adicionar size-comparison (JPEG < PNG) mas falhou na fixture sintética (blocos de cor uniforme: PNG comprime melhor que JPEG q=90 em flat regions). Removi e documentei no comment do teste que "size compaction is verified empirically on real movies".
   - Comments do header do arquivo atualizados (linhas 12-13, 19).

**Smoke test rodado em ciclo:**
- Primeira tentativa: 17/18 PASS (T8 falhou por causa da assumption de tamanho).
- Após fix: 18/18 PASS.

### Act 3: Re-run Roger Rabbit + cleanup + commit

1. `rm -rf kb/contact-sheets-clean` (legacy do v1 run).
2. Re-run `contact_sheet.py` com `--kb-force`. Cache hit, 162.8s extract (um pouco mais lento que v1 — 133s), tile labeled (3 PNG, 14.6+14.3+15.2 = 44.1 MB), tile KB (3 JPEG, 2.9+2.8+2.9 = **8.6 MB**). per-movie JSON re-emitido. JSONL appended.
3. **Compression ratio empírica medida em sheet 01:** 14.64 MB PNG → 2.87 MB JPEG = **80.4% redução**. Confirmou a expectativa de "~30% do tamanho" (na verdade saiu ainda melhor: ~20%).
4. **JSONL grew to 600 lines** (300 v1 + 300 v2 — gotcha do `--kb-force` documentada). Reconstrui o manifest.jsonl a partir dos per-movie JSONs (1 filme → 300 linhas únicas) via inline Python.
5. Commit `0c6fd49`: `feat(kb-export): kb sheet keeps numbering+TC+header (jpeg of labeled, ~80% lighter)`. 2 files changed, 49 insertions, 56 deletions.

### Act 4: User Q&A about pipeline behavior

Usuário pediu walkthrough completo do que acontece quando termina download. Forneci diagrama ASCII da cadeia `queue.py → aria2c → sheets_sweep.py → contact_sheet.py`, tabela de tempos esperados (12 min primeira run, 3 min cacheada), tabela de footprint (~62 MB/filme), variantes (Rust CLI, drop manual, --no-autosheets, --no-kb), e ⚠️ flagged o disk gate como bloqueador atual em produção.

## Decisions Made

- **Decision:** Bypassar sweep e usar `contact_sheet.py` direto pra primeiro test do KB — **Why:** disk gate em 9% livre bloqueia sweep; cache do scdet existe em `contact/`; quero validar o pipeline KB sem precisar liberar disco antes.
- **Decision:** Apontar `--out` pra `contact/` (existing) ao invés de `contact-sheets/` (vazio) — **Why:** reusar `scenes_raw_t8.txt` cache, poupar ~9 min de scdet em 4K x265.
- **Decision:** Usar defaults `cols=10 rows=10` (100 thumbs/sheet) ao invés do original `6×5` (30 thumbs/sheet) — **Why:** flag flag não-óbvia, esqueci. Sobrescreveu sheets 1-3 e deixou 4-10 órfãs em `contact/`. Pra teste de KB foi cosmeticamente irrelevante (manifest captura contagem real).
- **Decision (REVERTED de 001):** KB sheet agora preserva captions + header + TC, só JPEG — **Why:** usuário pediu explicitamente "uma versão mais leve do original". Schema de uso: humano vê labeled PNG no release, máquina ingere JPEG no kb/, ambos têm a mesma numeração. Reverte a doctrina original do snapshot 001 que dizia "Clean sheets: Same 10×(6×5) as labeled, minus captions+header".
- **Decision:** Renomear `kb/contact-sheets-clean/` → `kb/contact-sheets/` — **Why:** "clean" virou mentira após o refactor; nome correto é só "contact-sheets". Não colide porque o release usa `<release>/contact-sheets/`, não `kb/`.
- **Decision:** Remover param `clean=` de `tile_sheets()` ao invés de deixar dormente — **Why:** YAGNI / CLAUDE.md "Don't add features for hypothetical future requirements". Após refactor, `clean=True` virou dead code; remoção simplifica a função (params, body, hfont conditional). Mantém `ext=` pra controlar PNG vs JPEG.
- **Decision:** Remover branch `isinstance(src, Path)` em `tile_sheets()` — **Why:** após refactor, todos os callers passam Image objects (labeled). Branch Path virou dead code.
- **Decision:** T8 do smoke test não verifica mais "size smaller" — **Why:** falhou na fixture sintética porque PNG comprime melhor que JPEG q=90 em blocos de cor uniformes. JPEG-mais-leve é propriedade real-world (validado empiricamente em Roger Rabbit, 80.4% redução). Test agora verifica apenas dim parity (header preservado); compaction documentada no comment.
- **Decision:** Reconstruir `manifest.jsonl` do per-movie JSON ao invés de deixar 600 linhas duplicadas — **Why:** `--kb-force` faz append (gotcha documentada no snapshot 001). Como só tinha 1 filme e quero deixar pronto pro user ingerir, reconstruir é trivial e elimina a poluição. Validou também que o `--kb-rebuild-manifest` (P3 reservado) é factível.
- **Decision:** Commitar APENAS os 2 arquivos do refactor KB sheet, deixar `.gitignore`, `.DS_Store`, `kb/`, `logs/`, `CLAUDE.md`, `MEMORY_DEEP_001.md`, `docs/`, `.claude/commands/` no working tree — **Why:** scope discipline. Cada item demanda decisão separada do user. Não decidir por ele.
- **Decision:** Commit message convencional `feat(kb-export):` — **Why:** mudança de contrato comportamental (KB sheet agora tem labels), não pure refactor. Estilo dos commits anteriores: `feat(<scope>):`.

## Current State

**Working end-to-end (validado empiricamente em Roger Rabbit):**
- Pipeline `queue.py --wait <magnet>` → aria2c → sweep → contact_sheet.py com 3 outputs concorrentes:
  1. `release/contact-sheets/<slug>_sheet_NN.png` — humano, com captions (caption strip below + header)
  2. `kb/frames/<slug>/<slug>_frame_NNN.jpg` — RAG, frames pristine sem overlay
  3. `kb/contact-sheets/<slug>/<slug>_sheet_NN.jpg` — RAG, labeled JPEG (mesmo conteúdo do PNG, ~80% menor)
  4. `kb/per-movie/<slug>.json` + `kb/manifest.jsonl` — manifests
- Smoke tests: 12/12 sweep + 18/18 kb_export passando.
- Commit `0c6fd49` na main; 9 commits ahead da origin.

**Roger Rabbit state (single movie processed):**
- `downloads/Who Framed Roger Rabbit ...]/contact/` — 10 sheets (3 novas 10×10 + 7 órfãs 6×5 do run pré-pivô) + scdet cache 11 KB
- `downloads/Who Framed Roger Rabbit ...]/contact-sheets/` — VAZIO (sweeper não rodou pra ela)
- `kb/frames/who-framed-roger-rabbit-1988/` — 300 frames JPEG (7.8 MB)
- `kb/contact-sheets/who-framed-roger-rabbit-1988/` — 3 sheets JPEG labeled (8.6 MB)
- `kb/per-movie/who-framed-roger-rabbit-1988.json` — manifest completo
- `kb/manifest.jsonl` — 300 linhas únicas (reconstruído pós-`--kb-force`)
- Total kb/: **16.5 MB** pra Roger Rabbit

**Disco:** 91% usado, 9% livre (166 GB). **Abaixo do floor de 10% do sweep** — automação não vai disparar até liberar disco. Workaround: rodar `contact_sheet.py` direto.

**Working tree pendente (não tocado):**
- `M .gitignore` — adiciona `downloads/`, `.aria2*`, `__pycache__/`, `*.pyc` (housekeeping)
- `?? .DS_Store` — lixo Finder
- `?? .claude/commands/` — comandos custom (provavelmente commit)
- `?? CLAUDE.md` — instruções do projeto (provavelmente commit)
- `?? MEMORY_DEEP_001.md` — snapshot anterior (commit ou gitignore)
- `?? MEMORY_DEEP_002.md` — este snapshot
- `?? docs/brainstorms/`, `?? docs/plans/` — paper trail (commit)
- `?? kb/` — artefato derivado (gitignore)
- `?? logs/` — runtime (gitignore)

## Done (Cumulative)

- [x] `/pirata` skill spec'd (TR-100 monochrome, 12-branch menu)
- [x] Memory feedback saved: ANSI escapes don't render in Claude code fences
- [x] `scripts/queue.py` (existed pre-session 001; now tracked in git) — aria2c wrapper
- [x] `scripts/contact_sheet.py` — full pipeline (scdet → extract → label → tile)
- [x] Caption strip below thumb design (vs initial diagonal badges)
- [x] LLM-readable label fonts (auto-scaled with thumb width)
- [x] fps auto-detection via `probe_fps()`
- [x] Slug-prefixed sheet filenames (e.g., `who-framed-roger-rabbit-1988_sheet_NN.png`)
- [x] scdet result caching for fast re-runs
- [x] `scripts/sheets_sweep.py` — opportunistic sweeper, path-agnostic
- [x] sweep-level flock + security defenses (resolve+is_relative_to, --terminator, repr-sanitize, killpg)
- [x] `scripts/queue.py` `--autosheets`/`--no-autosheets` integration
- [x] `/pirata` skill panel rows: STATUS (LAST SWEEP, SHEETED, KB SIZE), DOCTOR (SWEEP, DL DIR, CONTRACT, KB DIR)
- [x] `scripts/tests/test_sweep.sh` — 12 assertions
- [x] `contact_sheet.py --kb-export` + `--kb-force` flags
- [x] `tile_sheets()` initially with `clean=True` mode (later removed in session 002)
- [x] `export_kb()` — frames JPEG + sheets JPEG + per-movie JSON + JSONL append
- [x] `sheets_sweep.py --kb`/`--no-kb` integration
- [x] `scripts/tests/test_kb_export.sh` — 18 assertions
- [x] Comprehensive docs: 1 brainstorm + 3 plans in `docs/`
- [x] 8 atomic git commits on main with conventional messages (session 001)
- [x] **First real-world KB export run validated on Roger Rabbit (session 002)**
- [x] **KB sheet refactor: clean re-tile → labeled JPEG (preserves captions + header, ~80% lighter)** (session 002)
- [x] **Dir rename: kb/contact-sheets-clean/ → kb/contact-sheets/** (session 002)
- [x] **`tile_sheets()` simplificado: dead code removido (clean= param + Path-input branch)** (session 002)
- [x] **Smoke test T8 atualizado: dim parity ao invés de "header absent"** (session 002)
- [x] **Manifest.jsonl deduplicado pós `--kb-force` via reconstrução do per-movie JSON** (session 002)
- [x] **Commit `0c6fd49`: feat(kb-export): kb sheet keeps numbering+TC+header** (session 002)

## Pending (By Priority)

### P1 — Urgent / Blocking

- [ ] **Liberar disco** — 9% livre, sweep não vai rodar real até passar de 10%. Roger Rabbit ocupa ~5 GB no `downloads/`; deletar libera margem.
- [ ] (Opcional) Migrar Roger Rabbit's existing `contact/` (mix de 6×5 antigas + 10×10 novas) → `contact-sheets/`. Sweep regeraria do zero quando rodar real (release sem `contact-sheets/*_sheet_*.png` é "unsheeted"). Alternativa: `mv` manual.

### P2 — Important

- [ ] Validar a cadeia AUTOMATIZADA end-to-end (queue.py --wait → sweep → contact_sheet → KB) em release fresca depois de liberar disco. Hoje só validei via invocação direta.
- [ ] Decidir RAG ingestion target (LlamaIndex / Haystack / knowledge-hub MCP / custom embedder). Manifest está pronto e agnóstico; entry-point é `kb/manifest.jsonl`.
- [ ] Considerar `--kb-caption` opt-in flag pra Moondream pass per frame. Schema reserva `caption: null`. Adiciona ~10min/filme.

### P3 — Nice to Have

- [ ] `scripts/sheets_sweep.py --kb-prune` — limpar entradas órfãs do KB após release deletada.
- [ ] `--kb-rebuild-manifest` utility pra regenerar `manifest.jsonl` de per-movie JSONs (handy após `--kb-force` rodar várias vezes — validado factível em session 002 via inline Python).
- [ ] launchd `.plist` pra auto-sweep periódico.
- [ ] IPTC/XMP metadata embedding via exiftool.
- [ ] Mega-sheet (single 300-thumb) como "movie fingerprint".
- [ ] `--kb-export` flag em `queue.py` pra expor controles de KB no top-level (hoje só via `sheets_sweep.py --kb`).
- [ ] `/pirata` skill UPDATE pra adicionar workflow de RAG-query.
- [ ] Cross-rip dedup (1080p vs 2160p colidem no slug; manifest's `source_file` é tiebreaker).
- [ ] Decidir `cols/rows` default — 10×10 (3 sheets) vs 6×5 (10 sheets). Originalmente 6×5; defaults atuais são 10×10. Talvez wire isso na skill `/pirata` ou em config.

## Technical Notes

**Stack** (unchanged from 001):
- Python 3.14.4 via pyenv
- ffmpeg-full 8.1 at `/opt/homebrew/opt/ffmpeg-full/bin/{ffmpeg,ffprobe}` (NOT default PATH, hardcoded como default em contact_sheet.py:36-37)
- Pillow 10.4.0
- aria2c via Homebrew
- macOS Darwin 24.6.0, Apple Silicon, /bin/bash 3.2.57 (frozen)
- 16 CPUs, ~36 GB RAM

**Config** (unchanged from 001):
- `~/.config/pirata/config.toml` — aria2.download_dir = `/Users/vidigal/claude-code/pirata/downloads`
- Env override: `AUTOSHEETS_MIN_SIZE_MB` (default 300 MB), `FFMPEG`, `FFPROBE`

**Compression empírica medida (Roger Rabbit, 4K x265 → 480px thumbs):**
- Labeled PNG sheet: ~14.6 MB (10×10 thumbs com header + caption strip)
- KB JPEG sheet (mesmo conteúdo): ~2.9 MB
- Ratio: **~80% redução** (PNG → JPEG q=90 em conteúdo natural rico)
- Frame JPEG q=90 individual: ~26 KB (480px wide)

**Pipeline timing v2 (Roger Rabbit, cache hit):**
- scdet: 0s (cached)
- extract 300 frames @ 6 workers: 162.8s (foi 133s no v1; variação normal pelo termal/load)
- label + tile labeled: ~5s
- KB (frames + sheets + manifests): ~10s
- **Total: ~3 min cacheado**

**Architecture (atualizada v2):**
```
queue.py [--wait] <magnet>
  ↓
aria2c → downloads/<release>/
  ↓ (if --wait + --autosheets)
sheets_sweep.py [--kb/--no-kb]
  ├─ DISK GATE: <10% livre → SKIP com warning
  ↓ (per qualifying release, if disk OK)
contact_sheet.py [--kb-export <kb>]
  ↓
release/<release>/contact-sheets/<slug>_sheet_NN.png   # labeled, humano
kb/frames/<slug>/<slug>_frame_NNN.jpg                  # raw JPEG, RAG
kb/contact-sheets/<slug>/<slug>_sheet_NN.jpg           # labeled JPEG (NEW: era "clean")
kb/per-movie/<slug>.json
kb/manifest.jsonl  (append)
```

## Key Files

**Scripts (Python):**
- `scripts/queue.py` — aria2c wrapper. Magnet validation, `--wait`/`--seed`/`--autosheets`/`--no-autosheets` flags, integration with sweeper post-aria2c.
- `scripts/contact_sheet.py` — main pipeline. ~485 lines (era ~500 antes do refactor v2). Argparse: positional `mkv`, flags `--out --threshold --floor --target --cols --rows --width --workers --title --keep-raw --kb-export --kb-force`. Helpers: `slugify`, `fmt_tc`, `fmt_tc_ff`, `probe_fps`, `probe_duration`, `parse_year_from_title`, `escape_movie_path`. Functions: `detect_scenes` (cached), `apply_floor`, `cap_target`, `_extract_one`, `label_frame`, `tile_sheets` (simplified v2: only Image inputs, always headed, ext-controlled), `export_kb` (signature changed v2: takes `labeled` + `header_font_size`).
- `scripts/sheets_sweep.py` — opportunistic sweeper. ~330 lines. Argparse: `--downloads --skip --dry-run --force --kb/--no-kb`. fcntl.flock LOCK_NB. Walk → filter → resolve+assert → run_contact_sheet (forwards `--kb-export`).

**Tests:**
- `scripts/tests/test_sweep.sh` — 12 assertions, ~150 lines bash. Hermetic.
- `scripts/tests/test_kb_export.sh` — 18 assertions, ~232 lines bash. Hermetic. Fixture: 7-color concat lavfi. T8 atualizado v2: dim parity ao invés de "header absent".

**Skill:**
- `.claude/skills/pirata-deck/SKILL.md` — main skill spec. PT-BR conversation, English technical terms.
- `.claude/skills/pirata-deck/references/menu-style.md` — TR-100 panel templates.

**Docs:**
- `docs/brainstorms/2026-04-24-kb-rag-multimodal-frames-requirements.md` — KB export brainstorm.
- `docs/plans/2026-04-24-001-feat-hunter-py-orchestrator-plan.md` — pre-existing plan (untouched).
- `docs/plans/2026-04-24-002-feat-auto-contact-sheets-plan.md` — sweeper plan (originally hook, pivoted post-doc-review).
- `docs/plans/2026-04-24-003-feat-kb-rag-multimodal-frames-plan.md` — KB export plan.

**Logs / runtime:**
- `logs/sheets_sweep.log` — append-only sweep log.
- `logs/.sheets_sweep.lock` — flock sentinel.
- `logs/kb_test_roger_rabbit.log`, `logs/kb_test_roger_rabbit_v2.log` — sessão 002 test runs.

**KB outputs (pirata workspace, gerados v2):**
- `kb/frames/who-framed-roger-rabbit-1988/*.jpg` — 300 frames
- `kb/contact-sheets/who-framed-roger-rabbit-1988/*.jpg` — 3 labeled sheets
- `kb/per-movie/who-framed-roger-rabbit-1988.json`
- `kb/manifest.jsonl` — 300 linhas

**Memory:**
- `~/.claude/projects/-Users-vidigal-claude-code-pirata/memory/MEMORY.md` — index.
- `~/.claude/projects/-Users-vidigal-claude-code-pirata/memory/feedback_ansi_in_code_fence.md` — feedback memory.
- `MEMORY_DEEP_001.md` — snapshot anterior.
- `MEMORY_DEEP_002.md` — este.

## Warnings & Gotchas

**Carregadas do snapshot 001:**
- ANSI escapes don't render in Claude Code fences.
- `scripts/queue.py` shadows stdlib `queue` — sys.path filter no top de cada script.
- macOS bash 3.2.57 frozen — sem `${VAR,,}`, sem associative arrays.
- `Path.resolve(strict=True)` BOTH sides em `is_relative_to` checks (macOS `/tmp → /private/tmp`).
- scdet output uma row por frame, maioria vazia — parser deve skip empty/comma-only.
- Argparse flag injection via filename — subprocess argv DEVE usar `--` terminator.
- Output dir é `contact-sheets/`, NÃO `contact/` (torrent payloads usam `contact/`).
- `--kb-force` rerun duplica linhas no JSONL (resolved via reconstrução em session 002, mas root cause persiste — `--kb-rebuild-manifest` ainda P3).
- scdet cache é per-threshold; mudar `--threshold` invalida cache.
- Disk gate skip on dry-run (by design).
- Sweep flock é `LOCK_NB`; concurrent sweeps loggam "already active" e exit 0.
- KB JSONL append-only sem rotation; gotcha pra ingest se rodar `--kb-force` repetidamente.
- Roger Rabbit's `contact/` tem mix antigo+novo (6×5 órfãs + 10×10 atuais). Sweep regeneraria do zero pra `contact-sheets/` quando rodar real.
- Moondream caption pass é opt-in; schema reserva `caption: null`.
- Manifest paths são RELATIVOS a `kb/` (rsync-safe).
- `sheets_sweep.py.slugify()` retorna HUMAN-READABLE TITLE, não URL slug; `contact_sheet.py.slugify()` é diferente. Don't unify sem checar callers.
- Pillow JPEG `optimize=True` é lento mas vale (~30% size reduction).

**Novas em session 002:**
- **Defaults `cols=10 rows=10` ≠ original `6×5`.** Run direto sem `--cols/--rows` cria 3 sheets de 100 thumbs ao invés de 10 sheets de 30. Cosmeticamente diferente do skill default; manifest captura layout real. Considere wirar default na skill ou em config.
- **`--out` apontando pra `contact/` (não `contact-sheets/`) reaproveita o cache do scdet** mas pollui o dir com sheets em layout possivelmente diferente do que o sweep geraria. Trade-off explícito pra primeiro test; em produção, preferir `--out contact-sheets/` (mesmo path que o sweep usa).
- **Compression JPEG vs PNG NÃO é universal.** Em conteúdo natural (Roger Rabbit), JPEG q=90 é ~80% menor. Em fixtures sintéticas (blocos de cor uniforme), PNG ganha (smoke test T8 inicialmente assumiu o contrário e quebrou). Smoke test agora só valida dim parity; size compaction é empírica.
- **`--kb-force` ainda duplica JSONL.** Reconstrução manual via Python inline funciona (validado em session 002). `--kb-rebuild-manifest` ainda é P3 mas a lógica está provada — basta wrappar o snippet inline numa função.
- **Sweep está bloqueado em produção (disco 9% livre).** A automação inteira (`queue.py --wait`) NÃO vai gerar sheets/KB até o disco passar de 10% livre. Workaround temporário: invocar `contact_sheet.py` direto. Documentar essa cadeia no `/pirata` DOCTOR panel é candidato pra próxima iteração.
- **MEMORY_DEEP_002 escrito antes do user committar `MEMORY_DEEP_001`.** Working tree tem ambos como untracked. Decidir convenção: commit ambos? Commit um e gitignore o outro? Por ora, ambos preserved.
