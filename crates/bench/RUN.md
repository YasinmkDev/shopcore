# ShopCore benchmark — run it on Windows (no dev tools needed)

This measures the Phase 0 gate numbers on the target PC. You need only
two files from the `shopcore-bench-windows` artifact (GitHub Actions →
`bench-windows` job): **`shopcore-bench.exe`** and this file.

## 1. Seed the dataset

Open a terminal in the folder with the exe and run:

```
shopcore-bench.exe seed --size full --db bench.db
```

- It prints the disk space needed (**~4 GB**) and the free space first,
  and refuses to start if there is not enough room.
- Seeding takes a while (tens of minutes on a slow disk). **If it is
  interrupted (power cut, closed terminal), just run the same command
  again** — it resumes where it stopped. Add `--fresh` to delete the
  database and start over.
- When it finishes it prints a `checksum:` line. Keep it: it proves
  the dataset is complete and identical to the reference.

## 2. Run the scenarios

```
shopcore-bench.exe run --size full --db bench.db --out results --target-pc
```

- `--target-pc` marks the numbers as the real gate run (without it
  they are labelled PROVISIONAL).
- This writes `results\results.json` and `results\results.md` and
  prints the summary. Scenarios 7–8 (startup / UI) show
  "not measured" until step 3 — that is expected.

## 3. Measure scenarios 7–8 in the app (debug route)

1. Start the ShopCore desktop app (`shopcore-desktop.exe`).
2. Press **Ctrl+Shift+B**. A "Bench debug" panel opens.
3. Click **Run scroll test** — it scrolls a 100k-row grid for 10 s
   and records frame p95, long tasks, and route-change time. Then
   click **Add 50 rows** (synthetic cart) and note the re-renders
   per row.
4. Click **Save results**. The panel shows the saved path — it is
   `%TEMP%\shopcore-ui-results.json` (paste `%TEMP%` in Explorer's
   address bar to find it).
5. Merge those numbers into the results folder:

```
shopcore-bench.exe merge-ui --results-dir results --ui-results %TEMP%\shopcore-ui-results.json
```

## 4. Send the results back

Zip the whole `results` folder (`results.json` + `results.md`) and
send it. It contains the machine record automatically (CPU model and
cores, RAM, disk type, Windows build, WebView2 version, power plan),
so nothing else needs to be typed out.

## Checks (optional)

- `shopcore-bench.exe verify --db bench.db` — re-proves the dataset
  (checksum + every per-SKU balance equals the sum of its movements).
  Prints `verify: OK` when it passes.
- If a run ever reports `database is locked`, that is a bug — stop and
  report it with the exact command you ran.
