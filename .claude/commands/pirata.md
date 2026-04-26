---
description: Abre o PIRATA Media Command Deck para buscar/enfileirar downloads de filmes, séries, anime, música, docs, lives, cursos e software
---

Abra o **PIRATA · Media Command Deck** usando a skill `pirata-deck`.

Renderize o **menu principal (picker de tipo de mídia)** com os 12 branches (0 HELP, 1 MOVIE, 2 SERIES, 3 ANIME, 4 MUSIC, 5 DOC, 6 LIVE, 7 COURSE, 8 SOFT, 9 STATUS, 10 DOCTOR, 11 QUEUE) conforme `.claude/skills/pirata-deck/SKILL.md` e `.claude/skills/pirata-deck/references/menu-style.md`.

Se o usuário já passou contexto após `/pirata` (ex: `/pirata dune 2`, `/pirata q "magnet:..."`, `/pirata 1b oppenheimer`), pule o menu e roteie direto pro workflow apropriado.
