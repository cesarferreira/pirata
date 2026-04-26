# Lessons

- In TUI download surfaces, only render numeric progress from downloader output. If progress is unknown, show stable `0.0%`, an empty bar, and status text; do not use animated placeholders or oscillating progress ratios.
- Keep TUI parallel downloads supported. Fix sluggishness by reducing redraw frequency, downloader output churn, and per-process priority/resource use before considering hard concurrency caps.
