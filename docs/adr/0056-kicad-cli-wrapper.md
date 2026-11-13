# ADR 0056: a `kicad-cli` wrapper for authoritative checks and exports

Status: accepted, implemented (2026-11-13). Follows ADR 0047 ("Next": "a `kicad-cli` wrapper for
authoritative DRC/ERC and STEP/IPC-2581/ODB++ export") and its Freerouting integration (phase 3,
`pcb.autoroute`), whose policy it repeats. No KiCad source was used; the command line and the JSON
report layout come from KiCad's published `kicad-cli` documentation and from memory of its report
schemas (see "Not verified").

## Context
Gusset's own DRC (ADR 0048, 0054) and ERC (ADR 0047 phase 2a) are deliberately a subset; KiCad's
are the reference users trust before ordering boards. KiCad is GPL (ADR 0001, 0047): it is never
linked, bundled or downloaded. `kicad-cli` can still be driven as a separate process the user
installed themselves, exactly as Freerouting is.

## Decisions
* **Policy (hard rule, as for Freerouting).** Gusset never downloads, installs or searches for
  KiCad: there is no `PATH` lookup and no probing of default install folders. Only a path the user
  types in the PCB panel's *KiCad (kicad-cli)* section is used (kept in the window preferences as
  `kicad_cli` and `kicad_cli_timeout`, handed to the session as `Session::kicad_cli`). Agents cannot
  set it. When it is unset the three commands refuse with `not_available` and a hint, and run
  nothing. Agents need the user's approval to run it (it starts a program), and to overwrite an
  existing export.
* **Commands (registry, ADR 0012).**
  * `pcb.kicad_drc {timeout_s?}`: `kicad-cli pcb drc --format json --severity-all --units mm
    --output <report> <board>`.
  * `sch.kicad_erc {timeout_s?}`: `kicad-cli sch erc --format json --severity-all --units mm
    --output <report> <root sheet>`; every sheet of the design is staged under its own relative
    name (names that are absolute, contain `..` or `\` are refused).
  * `pcb.kicad_export {format: step|ipc2581|odb, path, overwrite?, timeout_s?}`: `kicad-cli pcb
    export step|ipc2581|odb --output <file> <board>` (`--force` for STEP). The target must end in
    `.step`/`.stp`, `.xml` or `.zip` respectively (ODB++ is written as one zip) and follows the
    usual export rules (agents: inside the project, allow-listed extension, overwrite asks).
* **Running it.** The current board (with its `.kicad_pro`, and custom rules as `.kicad_dru`) or
  schematic is written from the session to a private temporary folder (0700 on Unix, removed
  afterwards) and only those copies are passed, so the user's files are never read or overwritten
  by KiCad. Arguments are a list (no shell); stdin is null; stdout and stderr go to files in the
  folder. The run is polled and killed at the time limit (setting, or `timeout_s`; at most one
  hour) and when either console file passes 4 MiB or the export file passes 512 MiB. A non-zero
  exit gives `io` with the last 400 characters of stderr (`stderr_user_text`); a run that writes no
  report or output gives `io`. Reports are read through the capped reader (32 MiB) and must be
  JSON. An exported file is copied into place through `durable_write`.
* **Report shapes.** Findings are put into Gusset's own shapes and every finding carries
  `"source": "kicad"` (so does the result); all KiCad text is `_user_text` and has control
  characters stripped:
  * DRC like `pcb.drc`: `violations[]`, each `{code, severity, message_user_text, items_user_text[],
    at [x, y] mm or null, source}`, plus `unconnected[]` and `schematic_parity[]` (same entry
    shape), `errors`, `warnings`, `unrouted` (the number of unconnected items), `more`, and
    `kicad_version_user_text`. At most 200 entries per list. `code` is KiCad's `type`; `at` is the
    first item with a position. Excluded findings and severity `ignore`/`exclusion` are left out.
  * ERC like `sch.erc`: `issues[]` with `{code, severity, message_user_text, sheet_user_text,
    items_user_text[], at, source}`, `errors`, `more`.
  * `coordinate_units` other than `mm` (`in`, `mils`) is converted to mm.
  The results are not merged with Gusset's own check: running `pcb.drc` and `pcb.kicad_drc`
  gives two reports, which differ and which the user can compare.
* **App.** The PCB panel's KiCad section holds the path, the time limit, a *KiCad design rule
  check* button and an export path with an *Export with KiCad* button (the file ending picks the
  format); the buttons are disabled until a path is set. Findings reuse the DRC marker list on the
  board, prefixed `[KiCad]`. The schematic panel shows a *KiCad electrical rule check* button when a
  path is set and lists the findings under it. The setting reaches the session before the command
  and whenever it changes.
* **MCP.** The commands are not in the `pcb`/`sch` toolsets (that would break the 600 bytes per
  tool average, `every_toolset_fits_the_budget`); they stay reachable through `commands` and `run`.
  Core list unchanged (31 tools, 17912 bytes); command tools 177, 579 bytes on average (was 577 at
  168 tools). The command-tool noise check in `tool_list_fits_its_budget` now looks for
  `"format":"` (the schema keyword) instead of any key named `format`, which the export command's
  `format` argument legitimately is.
* **Tests** (`gusset-commands/src/tests_kicad.rs`, Unix): fake `kicad-cli` shell scripts written
  by the test into a temp folder (nothing committed as a binary) that write canned reports: unset
  refuses (and nothing runs); agent approval; the report fields of a hand-written DRC and ERC
  report; the program runs inside a `gusset-kicad-*` folder that is gone afterwards and is never
  given a path in the project; the project folder is byte-identical afterwards; failure with
  stderr; no report; a report that is not JSON; timeout; endless output (`yes`) is stopped; a
  missing program; the three exports write their file, refuse an existing one unasked, refuse a
  wrong extension, and leave nothing when the program fails. Report parsing tests are portable.

## Not verified (no KiCad here)
The command lines and the report fields were written from documentation and memory and have not
been run against a real `kicad-cli`. In particular: the exact option names for `sch erc` and
`pcb export odb` in the KiCad version in use (9.x assumed), whether positions in ERC reports are in
mm with `--units mm`, the `excluded` flag, `severity` spelling (`error`/`warning`/`exclusion`),
and whether the output of `pcb export odb` is a `.zip` file with the default compression. A first
run with a real install will show; the parsing is tolerant (missing fields give empty or `null`
entries rather than errors), and the fix would be confined to `commands/kicad_cli.rs`.

## Not done
* No `pcb.kicad_drc` options (`--schematic-parity` needs the schematic file; custom severities).
* No other exporters (3D `glb`/`vrml`, `gerbers` through KiCad, `pos`, `svg`/`pdf` plots).
* Findings are not clickable yet (the marker list is text; the board shows the position).
* A running KiCad check blocks the session worker as `pcb.autoroute` does; there is no cancel
  button for it (the time limit applies).
* Windows: the code uses no Unix-only API (0700 is `cfg(unix)`), but the fake-program tests are
  Unix-only.
