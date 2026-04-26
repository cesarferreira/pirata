---
slug: "the-super-mario-galaxy-movie-2026"
title: "the-super-mario-galaxy-movie-2026"
year: null
frame_count: 300
first_tc: "00:00:42:05"
last_tc: "01:37:44:09"
first_t_s: 42.229
last_t_s: 5864.396
has_per_movie_json: true
fps: 24.0
runtime_s: 5881.05
source_size_bytes: 2519138614
extracted_at: "2026-04-26T05:00:50Z"
sheet_count: 10
json_frame_count: 300
scdet:
  threshold: 8
  floor_s: 4.0
  target: 300
---

# the-super-mario-galaxy-movie-2026

Slug: `the-super-mario-galaxy-movie-2026`

This is the contact-sheet derivative for the slug `the-super-mario-galaxy-movie-2026`.
It was extracted from a single source video and serves as a
pipeline-test artifact for the knowledge-hub ingest path.

## Pipeline metadata

- Title: the-super-mario-galaxy-movie-2026
- Year: None
- Frames extracted (manifest): 300
- First timecode: 00:00:42:05 (42.229 s)
- Last timecode:  01:37:44:09 (5864.396 s)
- FPS: 24.0
- Runtime: 5881.05 seconds
- Source size: 2519138614 bytes
- Frames in per-movie JSON: 300
- Contact sheets generated: 10
- Extracted at: 2026-04-26T05:00:50Z

## Scene detection (scdet) configuration

- threshold: 8
- floor_s: 4.0
- target: 300

## Caveats

- Title `the-super-mario-galaxy-movie-2026` matches the slug — the filename parser
  did not extract a human title. Unit 3 (IMDb enrichment)
  resolves this with `tconst`-anchored fields.
- Year is null — likely a parser miss on a dot-separated
  release filename. Unit 3 enrichment will populate this
  from IMDb.
- This wrapper predates Unit 3 (KB enrichment in the IMDb x
  pirata coupling plan). IMDb fields (tconst, rating, top_cast,
  akas, genres, director, plot) are NOT populated. Regenerate
  this export after Unit 3 ships.
- Pipeline-test export, not a semantic-recall-complete KB.
