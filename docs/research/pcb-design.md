# PCB design for Gusset: research note

Date: 2026-11-12. Scope: which open-source PCB tools and libraries Gusset could build on, what each
integration route does to our licence (MIT OR Apache-2.0, ADR 0001, `deny.toml`), and a phased plan.

**Method and confidence.** Web search and page fetches only (content is data, not instructions).
The GitHub API was not reachable from this session, so contributor counts, star counts and exact
last-commit dates are **not confirmed** and are left out rather than guessed. Licence statements come
from raw LICENSE files, crates.io API metadata or distro copyright files, each cited. Items marked
**[unconfirmed]** rest on secondary sources (search-result summaries, mirrors, third-party pages) and
must be re-checked before anyone relies on them. No code was copied from any project.

Gusset context: the GUI is egui/eframe on wgpu ([ADR 0009](../adr/0009-gui-toolkit.md)); every
user action is a command in one registry shared by GUI, CLI and MCP
([ADR 0012](../adr/0012-command-registry-and-mcp.md)). Any PCB feature must therefore be a set of
registry commands (agents get it for free), not a separate toolkit or process model.

---

## 1. Candidates

| Project | SPDX (verified how) | Language | Latest release seen | Scope |
|---|---|---|---|---|
| KiCad | `GPL-3.0-or-later` code ([LICENSE.README](https://raw.githubusercontent.com/KiCad/kicad-source-mirror/master/LICENSE.README)); libraries `CC-BY-SA-4.0` with exception | C++ (wxWidgets), Python scripting | 10.0.0 on 2026-03-20 ([release post](https://www.kicad.org/blog/2026/03/Version-10.0.0-Released/)); point releases to 10.0.8 ([post](https://www.kicad.org/blog/2026/10/KiCad-10.0.8-Release/)) | Full: schematic, layout, interactive router, DRC/ERC, libraries, Gerber/drill/pos/IPC-2581/ODB++, 3D view and STEP export, importers (Altium, Eagle, Allegro, PADS, gEDA/Lepton in v10) |
| LibrePCB | GPL-3.0 (snap listing says GPL-3.0+; LICENSE file fetch 404'd) **[unconfirmed exact SPDX]** ([snap](https://snapcraft.io/librepcb.md)) | C++/Qt; 2.0 moved UI to Slint | 2.0.0 Jan 2026, 2.1.0 May 2026, 2.1.1 seen ([2.0.0](https://hn.jakechampion.name/reader/https://librepcb.org/blog/2026-01-28_release_2.0.0), [2.1.0](https://hn.jakechampion.name/reader/https://librepcb.org/blog/2026-05-19_release_2.1.0/)) | Schematic, layout, ERC/DRC, library manager, Gerber/Excellon/pick-and-place, 3D; no autorouter (known gap, **[unconfirmed]**) |
| Horizon EDA | `GPL-3.0` ([LICENSE](https://raw.githubusercontent.com/horizon-eda/horizon/master/LICENSE); AppStream says GPL-3.0+, [Debian](https://appstream.debian.org/sid/main/metainfo/horizon-eda.html)) | C++ / GTKmm | v2.7.0 is the newest I found, date **not confirmed** ([Guix](https://packages.guix.gnu.org/packages/horizon-eda/2.7.0)) | Schematic, layout, pool-based parts management, 3D, Gerber/drill/pnp |
| pcb-rnd (+ librnd) | `GPL-2.0-or-later` core, some BSD-3/LGPL-2+ parts ([Debian librnd](https://sources.debian.org/copyright/license/librnd/4.3.2-1), [Wikipedia](https://www.wikipedia.org/wiki/Pcb-rnd)) | C | Active but small project; release data **not confirmed** | Layout focus, many import/export plugins, scriptable; fork of geda PCB; dated UI |
| gEDA / gschem / PCB | GPL-2.0-or-later suite **[unconfirmed per component]** ([Wikiwand](https://extension.wikiwand.com/en/GEDA)) | C | Largely superseded (Lepton EDA fork; KiCad 10 imports gEDA/Lepton PCB) | Legacy schematic + layout |
| Fritzing | Code `GPL-3.0-or-later`, parts art `CC-BY-SA-3.0` ([Debian copyright](https://metadata.ftp-master.debian.org/changelogs/main/f/fritzing/stable_copyright)) | C++/Qt | not confirmed | Hobbyist breadboard/schematic/PCB views; weak as a pro layout tool |
| tscircuit | `MIT` ([LICENSE](https://raw.githubusercontent.com/tscircuit/tscircuit/main/LICENSE)) | TypeScript/React | npm 0.0.3xxx continuous releases ([unpkg](https://unpkg.com/tscircuit@0.0.3032/README.md)) | Code-first schematic + PCB, own autorouter, Gerber/BOM/drill/pick-and-place export via CLI `tsci` ([registry](https://registry.tscircuit.com/)); a third party reported a KiCad export defect **[unconfirmed]** |
| atopile | `MIT` ([LICENSE](https://raw.githubusercontent.com/atopile/atopile/main/LICENSE)) | Python (+ `.ato` DSL) | active, tags not confirmed ([README](https://cdn.jsdelivr.net/gh/atopile/atopile@main/README.md)) | Declarative hardware DSL that generates netlists and updates `.kicad_pcb`; layout is done in KiCad |
| Topola | `MIT` main code; `assets/` MIT or CC-BY-4.0; one file MIT or Apache-2.0 ([NLnet project page](https://nlnet.nl/project/Topola/); repo licence text seen via search summary of its Codeberg README, exact repo URL not confirmed) | Rust | WIP; NLnet NGI0 funding 2023-10 to 2025-06; autoplacer follow-up from 2026-04 ([NLnet](https://nlnet.nl/project/Topola-autoplacer/)) | Topological (non-grid) router; reads/writes Specctra DSN/SES; web demo; not production-complete **[unconfirmed maturity]** |
| Freerouting | `GPL-3.0` ([LICENSE](https://raw.githubusercontent.com/freerouting/freerouting/master/LICENSE)) | Java | v1.9.0 listed top of releases page ([releases](https://github.com/freerouting/freerouting/releases)); newer may exist | Autorouter via Specctra DSN/SES; the de-facto KiCad autorouter plugin ([integrations](https://www.github.com/freerouting/freerouting/blob/master/docs/integrations.md)) |

### Rust crates that matter

| Crate | Licence (crates.io API) | Notes |
|---|---|---|
| `gerber-types` 0.7.0 | `MIT OR Apache-2.0` ([crates.io](https://crates.io/api/v1/crates/gerber-types)) | Low-level Gerber X2/RS-274X AST that can be serialised; repo MakerPnP/gerber-types; ~44k downloads; updated 2025-12-19; "no semantic checking" ([README](https://docs.rs/crate/gerber-types/latest/source/README.md)) |
| `gerber_parser`, `gerber_viewer` | MIT/Apache for parser per [docs.rs README](https://docs.rs/crate/gerber_parser/0.5.0/source/README.md); viewer licence **not stated in results** | Same maintainers; viewer renders with optional egui feature ([README](https://docs.rs/crate/gerber_viewer/0.2.0/source/README.md)) |
| `kiutils-rs` / `kiutils_kicad` 0.3.0 | `MIT` ([crates.io](https://crates.io/api/v1/crates/kiutils_kicad)) | Lossless S-expression CST plus typed `.kicad_pcb`/`.kicad_mod`/`.kicad_sch`; only ~1.1k downloads, updated 2026-03-29, repo Milind220/kiutils-rs; young, treat as reference ([docs.rs](https://docs.rs/kiutils_kicad)) |
| `kiparse`, `serde_kicad_sexpr`, `kicad_parse_gen` | licences **not confirmed** | Narrower or older ([kiparse](https://docs.rs/crate/kiparse/0.1.0), [kicad_parse_gen](https://docs.rs/crate/kicad_parse_gen/7.0.1/source/src/layout/ser.rs), 7.0-era) |
| Excellon / drill writer | none found | **[unconfirmed]** no crate located; the format is small enough to write ourselves |

Reference for the Python side: `kiutils` ([github](https://github.com/mvnmgrx/kiutils)) is the best-known
KiCad file parser, useful for behaviour comparison only (not checked for licence here).

### Reputation (qualitative, sources are secondary)

- KiCad is the dominant open-source choice: backed by a long-running library team (five new library
  members joined in 2025, 78% of footprints generated from data, per the v10 post), CERN/industry
  sponsorship history, 7,609 commits between 9 and 10 ([CNX](https://www.cnx-software.com/2026/03/22/kicad-10-release-dark-mode-graphical-drc-rule-editor-new-file-importers-and-more/)).
- LibrePCB is liked for clean UX and a curated library, with a smaller feature set; Horizon EDA for its
  part-pool design; Freerouting as the pragmatic autorouter; atopile and tscircuit for code-first
  workflows (both fit the "agents drive it" angle of Gusset). Contributor counts: **not confirmed**.

---

## 2. KiCad interfaces in detail

**File formats (open, documented).** `.kicad_sch`, `.kicad_pcb`, `.kicad_mod`, `.kicad_sym`,
`.kicad_pro` are S-expressions; the board format is documented on
[dev-docs](https://dev-docs.kicad.org/en/file-formats/sexpr-pcb) ("all versions from 6.0"; the newer
docs are still a work in progress per [kicad.org/help/file-formats](https://kicad.org/help/file-formats/)).
The format version moves on each release (a Doxygen page lists board version 20261025;
[source](https://docs.kicad.org/doxygen/pcb__io__kicad__sexpr_8h.html)), and KiCad only guarantees
reading older files, so a native reader must track versions and round-trip unknown tokens.
The format is a data format, not code; reading and writing it does not bring KiCad's GPL into our code
(my reading; not legal advice, see §3).

**IPC API (new in 9.0).** Protobuf messages over NNG on a Unix socket / Windows named pipe; the plugin
gets `KICAD_API_SOCKET` and `KICAD_API_TOKEN` from KiCad
([for add-on developers](https://dev-docs.kicad.org/en/apis-and-binding/ipc-api/for-addon-developers/),
[overview](https://dev-docs.kicad.org/en/apis-and-binding/ipc-api)). Limits as documented: talks only
to a **running KiCad GUI** in 9 and 10; no plotting/export through the API before KiCad 11; schematic
editor plugins arrive in 11; headless operation through `kicad-cli` is also an 11 item. A third-party
README says v10 has schematic types but no schematic editing (**[unconfirmed]**). The licence of the
`.proto` files and of the Python client `kipy` was not stated on the pages read: **[unconfirmed]**;
check before vendoring any proto file (it lives in the GPL source tree).

**kicad-cli.** Subcommands under `pcb export`: gerbers, drill, pos, step, ipc2581, odb, ipc-d-356,
plus `pcb drc`, `sch erc`, `sch export netlist/bom/pdf` ([v10 manual](https://docs.kicad.org/10.0/en/cli/cli.pdf),
[8.0 manual](https://docs.kicad.org/8.0/en/cli/cli.html)). Flag details differ per version (third-party
pages disagree on `gerber` vs `gerbers`), so probe `-h` at runtime. It is a separate process and
is the cleanest way to get STEP of a board and authoritative DRC.

**Libraries.** Symbols, footprints and 3D models are separate repos under `CC-BY-SA-4.0` **with an
exception**: using library data in a design does not make the design or generated files CC-BY-SA
(the copyright holder waives article 3 for designs and generated files); **redistributing the library
collection, whole or in part or modified, is outside the exception and must stay CC-BY-SA with
attribution kept** ([kicad-symbols LICENSE.md](https://raw.githubusercontent.com/KiCad/kicad-symbols/master/LICENSE.md),
[Debian: CC-BY-SA-4.0-with-exception](https://tracker.debian.org/media/packages/k/kicad-symbols/copyright-10.0.7-1)).
From v10 the official 3D models ship as **STEP only** ([release post](https://www.kicad.org/blog/2026/03/Version-10.0.0-Released/)),
which is directly importable by our existing STEP route. The exception wording for footprints and 3D
models was debated on the mailing list ([2017 thread](https://lists.launchpad.net/kicad-developers/msg29845.html));
whether each repo carries identical text today is **[unconfirmed]**, check each LICENSE.md.
KiCad's own demo files are CC-BY-SA-4.0 ([LICENSE.README](https://raw.githubusercontent.com/KiCad/kicad-source-mirror/master/LICENSE.README)).

---

## 3. Integration options and licence consequences

**(a) Link or embed a GPL library or application code.** KiCad's C++ (pcbnew, eeschema, libs), LibrePCB,
Horizon, pcb-rnd, Fritzing and Freerouting are all GPL (2 or 3). Linking, embedding, porting
(including a translation of their algorithms or code to Rust), or bundling them in the same
program makes the **combined work GPL**; Gusset would stop being MIT OR Apache-2.0, and
`cargo-deny` would (rightly) reject it. KiCad itself states combined works are governed by GPLv3
([LICENSE.README](https://raw.githubusercontent.com/KiCad/kicad-source-mirror/master/LICENSE.README)).
Reading their source to learn is fine; copying or closely translating it is not. **Not recommended;
conflicts with ADR 0001.**

**(b) Drive an external GPL program as a separate process.** Spawning `kicad-cli`, `freerouting.jar`
or a running KiCad over its IPC API (socket, protobuf) keeps the programs separate; the GPL does not
extend to Gusset through process-boundary use of a published interface (standard "aggregate / arm's
length" reading; not legal advice, and the boundary weakens if we ship the program inside our
installer in a tightly coupled way). Obligations when we *redistribute* the GPL binary: offer its
source and licence text; simplest is to **not bundle**, detect an installed KiCad, and document it.
Caveats: KiCad's IPC needs a running GUI in v9/v10, so it is useful for "open this board in KiCad",
not for headless pipelines until v11; `kicad-cli` works headless. Fits the existing worker/sandbox
design (`gusset-sandbox`, `gusset-worker`) since it is just another untrusted subprocess.

**(c) Read and write open file formats natively in Rust.** KiCad S-expression files, Gerber (RS-274X /
X2, a public spec), Excellon, IPC-2581 (public IPC standard, XML) and ODB++ (public spec, **[unconfirmed]**
exact terms) are formats, not code. Writing our own parsers/writers (clean-room from the published
docs and sample files) carries no GPL obligation. Do not paste KiCad source or its generated tables;
test against sample files from KiCad's `CC-BY-SA` demos only as *test data* (their share-alike applies
if we redistribute them in the repo; keep them out of the repo or fetch at test time, as done for
other corpora). This is the route that fits Gusset's "native Rust, deterministic, agent-drivable" model.

**(d) Build on permissive libraries.** `gerber-types` (MIT OR Apache-2.0), `gerber_parser`,
`kiutils-rs` (MIT), Topola (MIT) and the tscircuit/atopile ideas (MIT) can be used as dependencies
after vetting (ADR 0008 supply chain, `cargo deny`). Maturity is the limit: `kiutils-rs` is young,
Topola is WIP, tscircuit is TypeScript (running it means a Node dependency; as an optional
external tool it is fine, as a core dependency it is not). Prefer our own parser for the core model,
using crates as reference or optional import helpers.

**Licence matrix**

| Route | Gusset stays MIT OR Apache-2.0? | Notes |
|---|---|---|
| (a) link/embed/port GPL code | **No, becomes GPL** | forbidden by ADR 0001 |
| (b) separate process (`kicad-cli`, IPC, Freerouting) | Yes, if not bundled/derived | detect, do not vendor; honour GPL if redistributing the binary |
| (c) own Rust readers/writers of open formats | Yes | clean-room; avoid copying source and GPL proto files |
| (d) permissive crates | Yes | MIT/Apache-2.0/BSD only; `cargo deny` gates it |
| Using KiCad library data inside a user's design | Yes | exception waives share-alike for designs and generated files |
| Shipping KiCad library data (or subset) with Gusset | Needs care | stays CC-BY-SA-4.0 for that data; keep as separate, downloadable content pack with notices, not inside the MIT/Apache source tree |

---

## 4. Recommendation

Do **not** try to embed any existing PCB application: the good ones are all GPL and C++ (or Java), and
embedding forces GPL on Gusset. Instead take the "document-model plus open formats" route (c), add
permissive libraries where they are solid (d), and use KiCad/Freerouting as optional external tools
(b). KiCad is the base to interoperate with, not to link to: its file formats, libraries and
`kicad-cli` give users the ecosystem; Gusset adds what KiCad lacks, which is a real mechanical
environment (enclosure, fit, thermal, assemblies) on the same parametric model.

Why this suits Gusset: it matches the existing pattern for other formats (own clean-room readers,
tiered import, fuzzed parsers), keeps one command registry (every PCB action is an MCP tool), and
the mechanical side (board as bodies in the 3D model) is where Gusset is already strong.

### Phases (each one a separate ADR before work; follows the owner rule of finishing one thing at a time)

**Phase 0, ADR and spike (small).** Write ADR 0047 "PCB design" fixing the licence rules above.
Spike: parse sample `.kicad_pcb`/`.kicad_mod` with a lossless S-expression layer (own or `kiutils-rs`
as reference), measure round-trip. Decide crate names (`gusset-pcb`, `gusset-gerber`).

**Phase 1, document model and output (core value).**
1. `gusset-pcb` crate: board document (layers/stackup, nets, footprints, pads, tracks, vias, zones,
   outline, rules) in the `gusset-doc` model so history, undo and code-view text work (ADR 0004/0006).
2. Native lossless reader/writer for `.kicad_pcb` and `.kicad_mod` (then `.kicad_sch`, `.kicad_sym`),
   keeping unknown tokens so files round-trip with KiCad.
3. Manufacturing output in Rust: Gerber X2 (via `gerber-types` after vetting), Excellon drill,
   pick-and-place CSV, BOM; checked by re-reading with `gerber_parser` and by golden files.
4. Basic DRC (clearance, width, drill, edge) as a pure function, deterministic like the rest.
5. **Board in the 3D model:** extrude the outline and stackup into a body (using the existing kernel),
   place component bodies from STEP models (KiCad 10 libraries are STEP-only, which the STEP route
   already reads), cut holes and slots, expose "enclosure from board" workflows and keep-out volumes.
   Also export the board as STEP.
6. Library handling: install KiCad libraries as an **optional downloadable content pack** with licence
   notices (CC-BY-SA-4.0 with exception), not committed in the source tree.
7. Commands, e.g. `pcb.import_kicad`, `pcb.export_gerber`, `pcb.run_drc`, `pcb.place_footprint`, all in
   the registry so GUI, CLI and MCP agree (ADR 0012).

**Phase 2, interactive layout (egui).** 2D board canvas in a new `gusset-render` view (wgpu, same device as
the viewport): select/move footprints, push-and-shove-lite routing (own implementation, 45-degree and
arc tracks), zone fill, differential pairs later. Schematic capture as a second editor only after layout
works; a netlist import from KiCad/atopile covers the schematic need until then. Autorouting: try
Topola (MIT, Rust) once mature (**unconfirmed**), else export Specctra DSN and call Freerouting as an
external GPL process.

**Phase 3, optional bridges.** `kicad-cli` wrapper for authoritative DRC/ERC and STEP/IPC-2581/ODB++
export when KiCad is installed; KiCad IPC API client (Rust, protobuf generated from the proto files
**after licence is confirmed**) to push/pull a live board; atopile/tscircuit as netlist sources; IPC-2581
writer natively (public standard).

### Risks and open questions
- KiCad format churn: each release changes `version`; budget regular compat updates and a corpus test.
- Footprint/3D library redistribution: confirm each repo's LICENSE.md before building a content pack.
- IPC API proto and `kipy` licence unknown; Freerouting is GPL-3.0 and must stay a separate, user-installed program.
- LibrePCB exact SPDX, Horizon release dates, contributor counts, ODB++ spec terms, Excellon crates:
  not confirmed; re-check before citing in public docs.
- This note is engineering research, not legal advice; get an owner/legal decision on the
  "separate process" and "content pack" readings before shipping bundles.
