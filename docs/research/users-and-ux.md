# User and UI/UX research: Fusion, SolidWorks, Solid Edge, Onshape, FreeCAD, OpenSCAD

Status: research input, 2026-10-10. Author: research subagent. Not an ADR; decisions derived
from this go into ADRs.

## 0. Method, access and confidence

**What was reachable.** Only the web-search tool worked. Every direct page fetch failed:
`WebFetch` could not resolve news.ycombinator.com, hn.algolia.com, forum.freecad.org,
wiki.freecad.org, forum.onshape.com, onshape.com or reddit mirrors. Reddit is blocked
outright by the search provider ("domains are not accessible to our user agent").
So this report relies on search-result summaries and snippets. Each claim cites the URL
those snippets came from. The URLs were not opened directly, so quotes are as close to
verbatim as the snippets allowed.

| Cluster | Access | Notes |
|---|---|---|
| Reddit (r/cad, r/fusion360, r/SolidWorks, r/FreeCAD, r/openscad, r/3Dprinting, r/Onshape, r/MechanicalEngineering) | **Not accessible** | Blocked by the search provider. old.reddit.com fetch refused. One third-party mirror (r.datuan.dev) appeared once with no usable content. A third-party r/SolidWorks "snapshot" summary was found but could not be opened. **The Reddit picture comes only indirectly, from blogs and aggregators that summarise community opinion. Re-run this cluster when Reddit can be reached.** |
| Hacker News | Search snippets only (about 25 threads/comments) | Best-covered cluster |
| FreeCAD forum / GitHub / devtalk | Search snippets, GitHub issues | Forum pages could not be fetched. GitHub issues were used as the hard evidence for TNP status |
| Onshape forum | Search snippets (mostly the 2016 free-plan threads, mate-connector threads) | |
| Autodesk Fusion forum | Search snippets | Used for timeline, joints and marking-menu evidence |
| Blogs / review aggregators (G2, Capterra, TrustRadius, Fabbaloo, Hackster, LibreArts, Ondsel, vendor help) | Search snippets | G2 "pros/cons" summaries are AI-generated from reviews. Treat them as weak evidence |

**Frequency labels** used below: **Very common** = shows up in most clusters and in many
independent comments. **Common** = repeated in two or more clusters. **Occasional** = several
independent mentions. **One-off** = a single source. These are qualitative judgements from the
sampled material, not counts.

---

## 1. What people love, per tool

### Autodesk Fusion
- **Integrated CAD + CAM (+ simulation, PCB) in one app, concept to manufacture.** Very common
  among makers and CNC users ([SelectHub/MacHow2 summary via G2](https://g2.com/products/autodesk-fusion/reviews), [machow2](https://machow2.com/?p=31401)).
- **Free for personal use**, which is the main reason it is the default hobbyist recommendation on HN. One commenter calls it
  "still the most accessible entry to CAD for hobbyist makers" even with the 10-document limit
  ([HN 47084570](https://news.ycombinator.com/item?id=47084570)). Very common.
- **The parametric timeline**: users can "jump back into commands we've executed and adjust them, configure
  them, and even reorder them". Features can be suppressed and dragged within their dependency limits
  ([Autodesk blog](https://www.autodesk.com/products/fusion-360/blog/fusion-architectural-innovation/),
  [Fusion help: Timeline](https://help.autodesk.com/cloudhelp/ENU/Fusion-Assemble/files/ASM-TIMELINE.htm)). Common.
- **Approachable, "intuitive parametric modeling and sketches"** for personal projects
  ([G2 review](https://g2.com/survey_responses/autodesk-fusion-review-1109237)). Common.
- **Marking menu (right-click radial)**: users call it a time-saver. One user said that after it broke, they
  "would never take those right-click tools for granted again"
  ([Autodesk forum](https://forums.autodesk.com/t5/fusion-support-forum/right-click-menu-unable-to-select-item-since-april/td-p/10231722)). Occasional.
- Mixed history and direct modeling in one tool, plus T-spline "form" modeling (seen indirectly in the history-vs-direct threads
  ([Autodesk community](https://forums.autodesk.com/t5/fusion-design-validate-document/timeline-impact/m-p/11814303))). Occasional.

### SolidWorks
- **S-key shortcut bar**: a context-sensitive, customizable toolbar at the cursor that saves trips to the
  CommandManager. Resellers describe it as "exceptionally popular", and it changes per part, assembly, drawing or sketch
  ([CATI](https://www.cati.com/?p=118397), [SW help: Shortcut Bars](https://help.solidworks.com/2024/English/SolidWorks/Sldworks/Chunk150219028.htm),
  [Javelin](https://www.javelin-tech.com/blog/2017/07/solidworks-keyboard-shortcuts/)). Common in pro circles.
- **FeatureManager tree + rollback bar**: users can roll back, insert features while rolled back, and move the bar with the arrow keys
  ([SW help: Rollback bar](https://help.solidworks.com/2017/English/SolidWorks/sldworks/c_rollback_bar.htm)). Users praise
  editing an early feature's dimensions *in the context of the current model* without leaving it
  ([Autodesk forum comparison](https://forums.autodesk.com/t5/fusion-design-validate-document/timeline-impact/m-p/11814303/highlight/true)). Common.
- **Classic mates (coincident, concentric, distance) and degree-of-freedom thinking.** Ex-SW users keep asking
  Fusion for "elemental mates" ([Autodesk forum](https://forums.autodesk.com/t5/fusion-design-validate-document/where-are-the-constraints-mates/m-p/5549328/highlight/true)). Common.
- **Industry standard, depth of features, ease of use.** G2 rates SW above NX on ease of use (8.6 vs 8.1)
  ([G2 NX vs SW](https://www.g2.com/compare/nx-vs-solidworks)). Common.
- Affordable maker licence (3DEXPERIENCE SOLIDWORKS for Makers, US$48/yr per the official FAQ)
  ([3DS FAQ](https://www.3ds.com/store/faq/makers)). Occasional.

### Solid Edge
- **Synchronous technology**: direct, history-free edits with Live Rules that keep coplanar/concentric faces
  together. Reviewers say "it's so much faster to design this way" ([TrustRadius](https://trustradius.com/reviews/siemens-solid-edge-2019-02-22-11-53-34)).
  It also works on imported "dumb" solids, where Live Rules "finds the coplanar faces" so "a single move" shifts a series of ribs
  ([Machine Design](https://www.machinedesign.com/news/article/21816249/software-review-solid-edge-2008-with-synchronous-technology),
  [Siemens blog](https://blogs.sw.siemens.com/news/live-rules-introduction/)). Common (but much of the praise is vendor-leaning).
- **Free Community Edition**, "essentially the same as the commercial SE version". Its limits are a watermark on drafts and no file
  interchange with commercial SE ([model-engineer.co.uk](https://www.model-engineer.co.uk/?p=21353),
  [Siemens](https://resources.sw.siemens.com/en-US/download-solid-edge-community-edition)). Occasional.
- Ribbon "intuitive if you're familiar with ... Microsoft Office" ([G2](https://www.g2.com/products/solid-edge/reviews?page=4)). One-off/occasional.

### Onshape
- **Version control: branching, merging, micro-versions, effectively infinite undo.** One HN commenter:
  "very much based on concepts from GIT". An ex-SolidWorks user of 15 years says it is "much better for
  collaboration and version control" ([HN search cluster](https://news.ycombinator.com/item?id=44755110),
  [HN 18804393](https://news.ycombinator.com/item?id=18804393)). Very common.
- **Runs in the browser on anything, no install, no save button, real-time collaboration**: "completely platform agnostic",
  "a game changer" ([Capterra](https://www.capterra.com/p/164681/Onshape/reviews/), [G2](https://ai.g2.com/product/onshape)). Very common.
- **Stability and clear errors compared with Fusion and FreeCAD.** HN users who switched say fewer crashes and better performance
  ([HN 47498805 cluster](https://news.ycombinator.com/item?id=47498805), [HN 41517296](https://news.ycombinator.com/item?id=41517296)). Common.
- **Mate connectors / one-step Fasten mate** removes all 6 DOF. Users report "so many fewer clicks", and one long-time
  user thinks they are better "94% of the time" ([Onshape blog](https://www.onshape.com/cad-blog/onshape-assemblies-for-solidworks-users),
  [Onshape forum](https://forum.onshape.com/discussion/comment/91998)). Occasional.
- **FeatureScript (custom features), configurations, Variable Studio** ([Onshape help](https://cad.onshape.com/help/Content/VariableStudio/variable_studio_configurations.htm),
  [forum: most useful FeatureScripts](https://forum.onshape.com/discussion/31451/most-usefull-featurescripts)). Occasional (power users).
- **Multi-part Part Studio**: several parts modeled together in one feature list (a learning curve for SW users, then liked)
  ([Capterra](https://www.capterra.com/p/164681/Onshape/reviews/)). Occasional.

### FreeCAD
- **Free, open source, no licence or commercial restriction, offline, cross-platform.** This matters to people selling prints
  ([nozzledown](https://nozzledown.com/free-cad-software-3d-printing-2026/),
  [G2](https://www.g2.com/products/freecad/reviews)). Very common.
- **Python API, console and macros**, the most praised technical feature ("tailor the workflow")
  ([G2](https://www.g2.com/products/freecad/reviews)). Common.
- **Breadth through workbenches** (Arch/BIM, FEM, CAM, TechDraw, Assembly, add-ons) ([G2](https://www.g2.com/products/freecad/reviews)). Common.
- **1.0 / 1.1 progress.** "Today the experience for a hobby user is on par with other CAD programs"
  ([HN 42182022](https://news.ycombinator.com/item?id=42182022)). "Improvement over 1.0.x is massive", with on-canvas gizmos
  "at times actually ... easier to use than ... Fusion" ([HN 47521945](https://news.ycombinator.com/item?id=47521945)). One 2026 guide says 1.1 makes it
  "easy to recommend without an asterisk" ([siraya.tech](https://siraya.tech/blogs/news/best-free-cad-software-for-3d-printing)). Common, and newer than the complaints.
- **VarSets** (1.0+) preferred to spreadsheets for parameters: "work so much better in practice"
  ([HN search cluster](https://news.ycombinator.com/item?id=47521945)). Occasional.
- **Sketcher on-view parameters (type dimensions while drawing)** and the unified Dimension tool in 1.0
  ([FreeCAD blog](https://blog.freecad.org/2024/10/20/tutorial-on-view-parameters/), [LibreArts](https://librearts.org/2024/11/freecad-1-0/)). Occasional.

### OpenSCAD
- **Code-as-model**: plain text, parametric by default, reproducible, diffable. "I love the ability to parametrically
  define many of the objects I'm interested in printing", "a wicked awesome way to model"
  ([HN 13380441](https://news.ycombinator.com/item?id=13380441)). Very common among programmers and makers.
- **Customizer**: comments in the source become sliders and dropdowns for non-programmers, and it is scriptable from the CLI.
  It drove a huge Thingiverse ecosystem ([MakerBot Customizer docs](https://customizer.makerbot.com/docs),
  [scruss.com](https://scruss.com/blog/2018/07/14/thingiverse-customizer-a-tiny-guide/)). Common.
- Popular in the "maker/microcontroller/electronics world" ([HN 41515586](https://news.ycombinator.com/item?id=41515586)). Common.
- Simple mental model (CSG) that is "easier to keep momentum with" than build123d/CadQuery
  ([HN 41548945](https://news.ycombinator.com/item?id=41548945)). Occasional.
- Manifold backend (non-experimental since 2024.09; default in snapshots since Aug 2025) speeds up renders a lot
  ([OpenSCAD list](https://lists.openscad.org/empathy/thread/TMJEJCZINIJNYJX2YF7IDNBAPQY66KIF),
  [Fabbaloo](https://www.fabbaloo.com/news/openscad-embraces-multicolor-3d-design-with-new-geometry-engine)). Occasional.

---

## 2. Recurring complaints, per tool (with rough frequency)

### Fusion
| Complaint | Frequency | Evidence |
|---|---|---|
| Licensing/personal-use limits: 10 active editable docs (since Jan 2021), no commercial use, advanced CAM (3+2/4/5-axis) paid, simulation and generative design removed, rapids limited to cut feed. The STEP-export removal was reversed after backlash | **Very common**; anger peaked in 2020 and is still cited in 2026 guides | [Autodesk support](https://autodesk.com/support/technical/article/Fusion-360-Free-License-Changes), [Fabbaloo](https://www.fabbaloo.com/2020/09/autodesk-limits-functionality-in-free-version-of-fusion-360), [HN 24503950 "almost useless for hobbyists"](https://news.ycombinator.com/item?id=24503950), [Maslow forum](https://forums.maslowcnc.com/t/new-limitations-on-fusion-360-for-personal-use/14685), [nozzledown 2026](https://nozzledown.com/free-cad-software-3d-printing-2026/) |
| Cloud lock-in / offline restrictions / fear of losing access (a 2022 user quit after STL export would not work offline; claims about file expiry are disputed) | **Very common** | [HN 32489906](https://news.ycombinator.com/item?id=32489906), [HN 18805100](https://news.ycombinator.com/item?id=18805100), [G2](https://www.g2.com/products/fusion-360/reviews) |
| "Revoke features and jack the price" distrust of Autodesk (e.g., local simulation removed in 2022) | Common | [HN 32445203](https://news.ycombinator.com/item?id=32445203), [HN 32489906](https://news.ycombinator.com/item?id=32489906) |
| Slow / unstable on large assemblies; "not designed to handle large assemblies" | Common | [Chief Delphi](https://www.chiefdelphi.com/t/fusion-360-is-slow/373330), [Gartner](https://www.gartner.com/reviews/product/fusion-360), [SelfCAD blog](https://www.selfcad.com/blog/what-are-the-disadvantages-of-fusion-360) |
| Joints vs mates: ex-SW users "still hate joints" after 18 months. Hard to joint origin planes and multiple bodies in one component | Common (among SW migrants) | [Autodesk forum](https://forums.autodesk.com/t5/fusion-design-validate-document/joints-after-18-months-i-still-hate-joints/m-p/8631713), [forum](https://forums.autodesk.com/t5/fusion-design-validate-document/where-are-the-constraints-mates/m-p/5550723/highlight/true) |
| Timeline fragility: early edits break later features; editing rolls the whole model back so users lose context; history to direct switch is destructive ("All the features in the timeline are lost") | Common | [Autodesk forum: Timeline impact](https://forums.autodesk.com/t5/fusion-design-validate-document/timeline-impact/m-p/11814303), [Shapr3D discourse](https://discourse.shapr3d.com/t/direct-modeling-use-vs-parametric-aerospace-or-any-product-design/11671/7) |
| UI reshuffles (2019 "New UI is very confusing", disappearing toolbar) | Occasional | [Autodesk forum](https://forums.autodesk.com/t5/fusion-support-forum/new-ui-is-very-confusing/m-p/9013905/highlight/true), [Autodesk blog](https://www.autodesk.com/products/fusion-360/blog/ui-modernization-update/) |
| Marking-menu bugs (high-DPI, re-opening) | One-off/occasional | [Autodesk forum](https://forums.autodesk.com/t5/fusion-support-forum/marking-menu-bug/td-p/8850217) |

### SolidWorks
| Complaint | Frequency | Evidence |
|---|---|---|
| Cost and subscription push; "useful features ... remained the same, while the price has rocketed"; UK quote of about £4,500 + £1,000/yr | **Very common** | [Trustpilot](https://es.trustpilot.com/review/www.solidworks.com), [Capterra](https://capterra.com/p/93121/SolidWorks-Premium/reviews/), [Onshape forum](https://forum.onshape.com/discussion/comment/8360), [rfp.wiki](https://www.rfp.wiki/vendors/solidworks) |
| Crashes, especially large assemblies and STEP/non-native imports | Common | [rfp.wiki](https://www.rfp.wiki/vendors/solidworks), [Capterra](https://www.capterra.com/p/93121/SolidWorks-Premium/reviews/3829958/) |
| Windows-only: Mac users need Parallels on a specific validated build. RealView/OpenGL acceleration gets disabled on unlisted GPUs, and installs break | Common (Mac/Linux users) | [Ace Cloud](https://www.acecloudhosting.com/blog/does-solidworks-run-properly-on-mac-with-parallel/), [Parallels forum](https://forum.parallels.com/posts/948727/) |
| Licensing/support friction; 3DEXPERIENCE cloud/login skepticism; Makers licence files watermarked and incompatible with commercial | Occasional | [Trustpilot](https://es.trustpilot.com/review/www.solidworks.com), [Bambu forum](https://forum.bambulab.com/t/solidworks-cad-courts-our-community-with-maker-specific-cheap-software/80404), [Knee Slider](https://thekneeslider.com/3dexperience-solidworks-for-makers-puts-pro-design-tools-on-your-desktop) |
| Heavy hardware requirements | Occasional | [rfp.wiki](https://www.rfp.wiki/vendors/solidworks) |

### Solid Edge
| Complaint | Frequency | Evidence |
|---|---|---|
| Steep learning curve, overwhelming UI; ordered vs synchronous modes confuse people | Common | [G2](https://www.g2.com/products/solid-edge/reviews?page=5), [AWS Marketplace reviews](https://aws.amazon.com/marketplace/reviews/reviews-list/prodview-glj5jqmflsiem?page=12) |
| Live Rules "makes assumptions that can occasionally be wrong", "too much for beginners" (Siemens' own blog) | Occasional | [Siemens blog](https://blogs.sw.siemens.com/solidedge/synchronous-technology-and-live-rules/) |
| Large-assembly slowness, crashes | Occasional | [G2](https://www.g2.com/products/solid-edge/reviews?page=4) |
| Windows-only; Community Edition non-commercial and watermarked; confusing old installers on Siemens sites | Occasional | [model-engineer.co.uk](https://www.model-engineer.co.uk/?p=21353), [cadforum](https://cadforum-net.dezignstuff.com/viewtopic.php?p=7544) |
| Small community/mindshare (absent from most hobbyist "best free CAD" lists) | Occasional (inferred) | [itechguides 2026](https://www.itechguides.com/our-4-favorite-free-cad-programs-for-2026/) |

### Onshape
| Complaint | Frequency | Evidence |
|---|---|---|
| Free plan: all docs public, no private docs, non-commercial. Since 2016 there has been no affordable middle tier (Standard is about US$1,500/user/yr). The vendor told hobbyists who want private data "we may not be a fit" | **Very common** | [Onshape help](https://cad.onshape.com/help/Content/Plans/managing_your_onshape_account_and_subscription.htm), [Onshape forum 2016](https://forum.onshape.com/discussion/comment/23090), [HN 24504393](https://news.ycombinator.com/item?id=24504393), [toolradar](https://toolradar.com/tools/onshape/pricing) |
| Internet dependence ("if you're offline you're doomed") and cloud data custody | **Very common** | [G2](https://ai.g2.com/product/onshape), [Capterra](https://www.capterra.com/p/164681/Onshape/reviews/?page=5), [HN 18804393](https://news.ycombinator.com/item?id=18804393) |
| Large-assembly lag | Common | [rfp.wiki](https://www.rfp.wiki/vendors/onshape), [Capterra](https://www.capterra.com/p/164681/Onshape/reviews/?page=5) |
| Weaker drawings, sheet metal and CAM than SW | Occasional | [G2](https://ai.g2.com/product/onshape), [Capterra](https://www.capterra.com/p/164681/Onshape/reviews/) |
| Mate connectors hard to learn for SW/Creo users; assemblies have no default planes | Occasional | [Onshape forum](https://forum.onshape.com/discussion/5050/why-is-the-ability-to-use-and-mate-to-the-planes-used-in-part-construction-missing-in-assemblies) |
| "Does not allow engineers to invent their own workflows" (owned by PTC, opinionated) | One-off | [HN 46791911](https://news.ycombinator.com/item?id=46791911) |

### FreeCAD
| Complaint | Frequency | Evidence |
|---|---|---|
| **UI/UX awkward, steep learning curve, "FreeCAD's way"**: "The UX is the worst I've seen", "UX still sucks", "Ondsel is FreeCAD without the sadistic UX". Still the #1 complaint after 1.1 | **Very common** | [HN 47498805](https://news.ycombinator.com/item?id=47498805), [HN 42182022](https://news.ycombinator.com/item?id=42182022), [HN 40430677](https://news.ycombinator.com/item?id=40430677), [Capterra](https://www.capterra.com/p/167527/FreeCAD/reviews/), [Ondsel blog](https://ondsel.com/blog/freecad-breaking-open-source-ux-curse) |
| **Topological naming problem (TNP)**, see the status below | Very common before 1.0; **common but declining** now | below |
| Workbench confusion (Part vs PartDesign vs Sketcher vs Draft, Body rules), confusing names | Common | [cadartifex](https://www.cadartifex.com/blogs/freecad-part-design-vs-part-workbench), [GitHub discussion #10205](https://github.com/FreeCAD/FreeCAD/discussions/10205), [devtalk](https://devtalk.freecad.org/t/community-uiux-improvement-project/59206) |
| Crashes, weird solver behaviour, "bizarre shapes due to numerical issues", sketch constraints "like pulling teeth", 3D selection problems | Common | [HN 47521945 cluster](https://news.ycombinator.com/item?id=47521945), [HN 18804393 cluster](https://news.ycombinator.com/item?id=41516313) |
| Slow on complex models | Occasional | [G2](https://www.g2.com/products/freecad/reviews_and_filters) |
| UI change process "devolves into bike-shedding"; Ondsel shut down | Occasional | [Ondsel blog](https://ondsel.com/blog/freecad-breaking-open-source-ux-curse), [Ondsel goodbye](https://ondsel.com/blog/goodbye/) |

**TNP status, 1.0 → 1.1.**
- *Official:* 1.0 (Nov 2024) is the first stable release with the new naming algorithm (based on realthunder's work).
  It "will be applied to more parts of FreeCAD", with "more automatic and assisted repair" to come
  ([FreeCAD wiki](https://wiki.freecad.org/Topological_naming_problem/en), [Hackster](https://hackster.io/news/freecad-hits-1-0-after-two-decades-finally-sees-an-end-to-the-toponaming-problem-7e7ac2df3e63)).
- *Users:* the fix mainly covers **sketch attachment**, and "chamfers and fillets" still struggle
  ([Lemmy comment](https://blendit.bsd.cafe/comment/1074606)). Another user: "not entirely solved, but I've slowly learned ... to be
  better at constructing models" ([LinuxCNC forum](https://forum.linuxcnc.org/31-cad-cam/54697-freecad-1-0-who-s-using-it)). A 1.1 user
  had "not encountered the same issue" ([HN](https://news.ycombinator.com/item?id=47097446)).
- *Hard evidence of residual problems in the 1.x line:* fillets and chamfers break when upstream features add or remove edges
  ([#26826](https://github.com/freecad/freecad/issues/26826)); a pad-length change breaks fillets, sketches and pockets
  ([#29154](https://github.com/FreeCAD/FreeCAD/issues/29154)); fillet edges disappear ([#25760](https://github.com/FreeCAD/FreeCAD/issues/25760));
  "?" broken references ([#19356](https://github.com/FreeCAD/FreeCAD/issues/19356)); pre-1.0 files pick the wrong edges
  ([#18372](https://github.com/FreeCAD/FreeCAD/issues/18372)).
- *Perception now:* TNP has moved from "the number one, absolute must fix problem" ([HN 2022](https://news.ycombinator.com/item?id=32794625))
  to "mostly mitigated, edge-based features still fragile". UI and stability have replaced it as the top complaint.
  **Lesson for us:** persistent naming of edges and faces through fillet and chamfer is the part that remains hard. Design for it from day one, and
  build a "repair broken reference" UX.

### OpenSCAD
| Complaint | Frequency | Evidence |
|---|---|---|
| **No native fillets/chamfers; cannot reference edges or faces of earlier results** (mesh CSG, not B-rep). BOSL2 helps but is expensive and has edge cases | **Very common** | [HN 42429837](https://news.ycombinator.com/item?id=42429837), [HN 41548945](https://news.ycombinator.com/item?id=41548945), [OpenSCAD list](https://lists.openscad.org/empathy/thread/OUIOVDBUO2HEQCN3VVZZTGF2HGHT4R4B), [Bambu forum](https://forum.bambulab.com/t/parametric-model-maker-model-times-out-help/211145) |
| No STEP import/export (mesh-only output) | Common | [HN 41515586](https://news.ycombinator.com/item?id=41515586) |
| Slow render (CGAL), preview differs from final render (improved greatly by Manifold) | Common (historical) | [HN 18429758](https://news.ycombinator.com/item?id=18429758) |
| Language limits; nested translate() "hacky"; no interactive selection | Occasional | [HN 13380441](https://news.ycombinator.com/item?id=13380441), [HN 28907226](https://news.ycombinator.com/item?id=28907226) |
| 3MF multi-object export only via the experimental lazy-union feature; import crash | One-off | [OpenSCAD list](https://lists.openscad.org/empathy/thread/D6KV3ZLXHLBHSITSQ5GPUZUKHURU4ABE) |

**Cross-tool meta-complaints (very common):** vendor control of data and licences (Fusion, Onshape, SW). The HN consensus is
"Fusion if you accept cloud/account; Onshape if you accept public; FreeCAD if you prioritise open/offline and tolerate roughness"
([HN cluster](https://news.ycombinator.com/item?id=24504393)). **No tool combines free + private + offline + polished +
commercially usable.** That gap is our positioning.

---

## 3. Daily vs rare feature use, by user segment

There is no survey that ranks CAD features by use (searched: [3dprint.com/Sculpteo](https://3dprint.com/133924/sculpteo-report-hobbyists/), [UMP study](https://journal.ump.edu.my/index.php/jmes/article/view/8985) cover slicers, not CAD).
The table below is **inferred** from tutorials, complaint topics and segment descriptions cited above, and is labelled as inference.

| Feature | Hobbyist / 3D printing | Professional mech. engineer |
|---|---|---|
| Sketch (line/rect/circle/arc/slot), dimensions, constraints | Daily | Daily |
| Extrude/pad, pocket/cut, revolve | Daily | Daily |
| Fillet, chamfer | Daily (print-friendly edges) | Daily |
| Shell, holes (incl. threads / countersink), text/emboss | Weekly | Weekly (hole wizard daily) |
| Mirror, linear/circular patterns | Weekly | Daily |
| Parameters / variables | Weekly (Customizer-style "make it 5 mm bigger") | Daily |
| Booleans / multi-body, split body (fit on print bed) | Weekly | Occasional |
| STL / 3MF export | Daily (the end of every workflow) | Occasional |
| STEP import (vendor parts, McMaster etc.) | Weekly | Daily |
| Measure / section view | Daily | Daily |
| Assemblies / joints / mates | Occasional (multi-part prints, enclosures) | Daily |
| Drawings (2D, GD&T, BOM) | Rare | Daily |
| Configurations / variants | Rare | Weekly |
| Sweep / loft | Occasional | Weekly |
| Sheet metal | Rare | Weekly (in sheet-metal shops: daily) |
| Surfacing | Rare | Occasional (consumer products: daily) |
| Direct edit of imported geometry | Occasional | Weekly |
| CAM | Occasional (CNC hobbyists heavy users, cf. Fusion CAM loyalty) | Domain-specific |
| FEA simulation | Rare | Occasional |
| Rendering | Rare | Occasional |
| Version control / branching | Rare (wanted by programmers) | Weekly in teams (Onshape users rate it a "killer feature") |
| Scripting/API | Rare except the OpenSCAD/programmer segment (daily for them) | Occasional (automation) |

Evidence anchors: CAM is "2.5/3-axis enough" for most hobbyists ([hobby-machinist](https://hobby-machinist.com/threads/fusion-360-defunctioning.86864/page-7));
Autodesk says the features it cut from the personal tier were "almost all used only by advanced users" ([Maslow forum](https://forums.maslowcnc.com/t/new-limitations-on-fusion-360-for-personal-use/14685?page=2));
version control is "the killer feature" for teams ([HN](https://news.ycombinator.com/item?id=44755110)); STL/3MF export, STEP and active-document
limits are what hobbyists fought over in 2020 ([Autodesk forum](https://forums.autodesk.com/t5/fusion-support-forum/please-autodesk-don-t-limit-the-personal-licenses/m-p/9750480)).

---

## 4. UI/UX: praise and hate per interface

| Tool | Praised | Hated |
|---|---|---|
| Fusion | Horizontal timeline that is visible all the time; marking menu; inline dimension entry in sketches; one app for design → CAM ([Autodesk help](https://help.autodesk.com/cloudhelp/ENU/Fusion-Assemble/files/ASM-TIMELINE.htm), [forum](https://forums.autodesk.com/t5/fusion-support-forum/right-click-menu-unable-to-select-item-since-april/td-p/10231722)) | Editing a feature rolls back the whole model and loses context; destructive history↔direct switch; "capture design history" setting hard to find; workspace/menu reshuffles; component/body/joint model confusing to SW users; lag tied to cloud autosave ([forum](https://forums.autodesk.com/t5/fusion-design-validate-document/timeline-impact/m-p/11811389/highlight/true), [chiefdelphi](https://www.chiefdelphi.com/t/fusion-360-is-slow/373330)) |
| SolidWorks | S-key context bar; CommandManager tabs per context; FeatureManager tree with rollback bar and in-context editing; Confirmation Corner, "D" to bring it to the cursor; keyboard shortcuts everywhere ([Javelin](https://www.javelin-tech.com/blog/?p=65848), [SW blog](https://blogs.solidworks.com/tech/2018/01/useful-keyboard-shortcuts-workflow-customizations-solidworks.html)) | Dated, dense, Windows-only; heavy; crash-prone on big data |
| Solid Edge | Ribbon familiar to Office users; steering wheel / Live Rules for direct edits | "Overwhelming", hard-to-find functions; two modelling modes (ordered/synchronous) add cognitive load ([AWS reviews](https://aws.amazon.com/marketplace/reviews/reviews-list/prodview-glj5jqmflsiem?page=12)) |
| Onshape | Speed of getting started (URL, no install); no save button; version graph; clean, consistent feature dialogs; fewer mates | Offline impossible; multi-part studio and mate connectors take relearning; large-assembly lag |
| FreeCAD | 1.0: light/dark themes, a standalone dockable task panel, on-view sketch parameters, a unified Dimension tool (M cycles modes); 1.1: transparent previews of PartDesign operations, interactive draggers, a Transform tool with numeric input, Preferences search, a new navigation default with SW/NX-like presets, a theme editor ([LibreArts](https://librearts.org/2024/11/freecad-1-0/), [AlternativeTo 1.0](https://alternativeto.net/news/2024/11/freecad-1-0-launches-with-enhanced-ui-ux-built-in-assembly-workbench-and-tnp-fixes/), [AlternativeTo 1.1](https://alternativeto.net/news/2026/3/freecad-1-1-adds-three-point-lighting-part-design-previews-and-major-fem-upgrades), [linuxadictos](https://en.linuxadictos.com/FreeCAD-1.1-introduces-profound-improvements-in-visualization-and-interaction-with-transparent-previews-and-three-point-lighting.html)) | Workbench switching and duplicate tools across workbenches; inconsistent toolbars (a ribbon was proposed but called "contentious"); confusing names; modal task panel; selection and constraint frustration; errors that do not say what broke ([GitHub #13775](https://fr.github.com/FreeCAD/FreeCAD/discussions/13775), [#10205](https://github.com/FreeCAD/FreeCAD/discussions/10205)) |
| OpenSCAD | Editor + preview, F5/F6, Customizer panel generated from comments | No direct manipulation, can't click a face to refer to it, slow full renders |

### 4.1 Derived design rules for our app (actionable)

1. **One modelling environment, no workbenches.** Tools appear by *context* (sketch / part / assembly / drawing), never by
   a user-selected "workbench". Each operation exists exactly once. (FreeCAD workbench pain; SW CommandManager context tabs.)
2. **A command palette with fuzzy search (Ctrl+K / `/`)** that lists every command, setting and MCP-exposed action, with its
   shortcut shown. (Discoverability complaints for Fusion/Solid Edge; FreeCAD 1.1 added Preferences search.)
3. **A cursor-local, context-sensitive quick bar (S key) and a radial marking menu on right-drag.** Both are user-customizable and
   respond instantly (radial menus must never misfire on a quick release; see the Inventor/Fusion bugs).
4. **The history tree and timeline are one model shown two ways**: a vertical tree (SW) and an optional horizontal timeline (Fusion),
   both with a draggable rollback marker, suppress, reorder (within dependency limits) and dependency highlighting.
5. **Edit-in-context by default:** editing an earlier feature's parameters shows the *full current model* with a live preview of the
   result. Roll back only when editing a sketch, and then ghost the downstream geometry. (Fusion "lose context" complaint vs SW.)
6. **Transparent "before / after" operation previews** that update live while parameters change (FreeCAD 1.1's best-received change).
7. **Type-while-drawing dimensions** (on-view parameters) and expressions in *every* numeric field (`=width/2 + 3mm`, units allowed).
8. **First-class named parameters** in a variables panel, VarSet-style rather than a spreadsheet. A parameter can be exposed as a "Customizer"
   slider/dropdown for end users. (FreeCAD VarSets praise; OpenSCAD Customizer.)
9. **Robust references are a UX feature:** when a reference breaks, never silently pick another edge. Mark the feature, show the
   old reference ghosted, and offer one-click "re-pick" or "accept suggestion". (Lessons from FreeCAD TNP issues #19356 and #26826.)
10. **Errors say what, where and how to fix**, link to the offending feature/edge, and never leave the model in an
    undefined state. (Onshape praised for "clearer errors"; FreeCAD "bizarre shapes".)
11. **Infinite undo through micro-versions, plus named versions and branches/merge** in a local-first store (git-like), with
    no "save" anxiety: autosave is continuous and local. (Onshape's killer feature, without the cloud.)
12. **Local-first, offline-always, open file format, no account.** Cloud/collaboration is opt-in and never required for export.
    (Very common Fusion/Onshape complaint.)
13. **Never gate exports.** STEP, STL, 3MF and DXF are always available; 3MF is the default for printing. (Fusion 2020 backlash.)
14. **A direct-edit layer on top of history:** push/pull faces, move holes and offset faces on any body, including imported "dumb" STEP solids,
    with recorded features (Live-Rules-style coplanar/concentric hints shown as toggleable chips, never silently applied).
    No destructive history↔direct mode switch. (Solid Edge praise; Fusion destructive switch.)
15. **Assemblies: mate-connector placement plus classic mates.** Offer a one-step "fasten" via connectors (Onshape) *and*
    elemental coincident/concentric/distance mates with a visible DOF indicator (SW). Assemblies get default origin planes.
16. **Multi-body part studio** (several bodies in one feature list) with a clear body/part/component vocabulary in a
    glossary tooltip. Avoid FreeCAD's Part/Body naming confusion and Fusion's component/body confusion.
17. **Code is a peer of the GUI:** every GUI action maps to a script/MCP call, shown in a "recorded script" pane. A
    text model (OpenSCAD-like) can be authored, diffed and round-tripped, *with* B-rep edge selection by stable name.
18. **Fast and calm at scale:** viewport interaction under 16 ms, rebuilds incremental and cancellable, large assemblies loaded lightweight.
    (Fusion/Onshape/SW all criticised for large assemblies.)
19. **Navigation presets** (our default plus SW, Fusion, Onshape/FreeCAD-style mouse mappings) chosen on first launch, and
    keyboard-remappable shortcuts that ship with familiar defaults. (FreeCAD 1.1 added SW/NX navigation presets.)
20. **A stable layout across releases:** no surprise reshuffles. UI changes go through a written design guide and are
    previewed behind a toggle first. (Fusion 2019 "new UI is very confusing"; FreeCAD's design-guide initiative.)
21. **Light/dark themes from design tokens**, HiDPI-correct, cross-platform parity (Win/macOS/Linux, including Wayland).
    (SW Windows-only pain; FreeCAD 1.1 Wayland and token-based theming.)
22. **Selection that never fights the user:** pre-highlight, a "select other" picker for overlapping faces/edges, temporary
    see-through of occluders (FreeCAD 1.1), and selection filters on the quick bar.

---

## 5. Feature matrix

Legend: Y = yes, P = partial / via add-on / limited, N = no. Our tag: **MVP** (first usable release), **v1**, **later**.
Vendor capability cells come from product knowledge plus the sources cited in sections 1–2. The cells marked † are
backed by a source in this document: Onshape CAM/Render/Simulation ([Onshape roadmap](https://www.onshape.com/en/blog/new-cad-features-2023-roadmap),
[Render Studio](https://www.onshape.com/en/blog/onshape-launches-render-studio),
[DE247 simulation](https://www.digitalengineering247.com/article/ptc-adds-designer-targeted-simulation-to-cloud-cad-onshape)); Solid Edge CAM Pro / Generative
([Siemens CAM Pro](https://blogs.sw.siemens.com/wp-content/uploads/sites/8/2024/10/Siemens-SW-Solid-Edge-CAM-Pro-FS-78035-D16.pdf),
[Generative Design Pro](https://solidedge.siemens.com/en/siemens-sw-solid-edge-generative-design-pro-fs-73871-d7)); OpenSCAD no STEP
([HN](https://news.ycombinator.com/item?id=41515586)); Fusion personal-tier limits ([Autodesk](https://autodesk.com/support/technical/article/Fusion-360-Free-License-Changes)).

| Feature | Fusion | SW | SE | Onshape | FreeCAD | OpenSCAD | Ours | Justification |
|---|---|---|---|---|---|---|---|---|
| 2D sketch tools (line, arc, circle, rect, slot, spline, offset, trim, project) | Y | Y | Y | Y | Y | P (2D primitives, no sketcher) | **MVP** | Every workflow starts here |
| Geometric + dimensional constraints, DOF display | Y | Y | Y | Y | Y | N | **MVP** | Core of parametric intent; FreeCAD solver complaints show quality matters |
| On-view dimension entry, expressions in fields | Y | Y | Y | Y | Y (1.0+) | N/A | **MVP** | Cheap, very high UX value |
| Extrude / revolve | Y | Y | Y | Y | Y | Y (linear/rotate_extrude) | **MVP** | Daily for all |
| Sweep / loft | Y | Y | Y | Y | Y | P (BOSL2) | **v1** | Weekly for pros, occasional for hobbyists |
| Fillet / chamfer | Y | Y | Y | Y | Y (TNP-fragile) | P (BOSL2 only) | **MVP** | Daily for printing; OpenSCAD's top gap |
| Shell | Y | Y | Y | Y | Y | P | **MVP** | Enclosures, very common in printing |
| Draft | Y | Y | Y | Y | Y | N | **v1** | Molding/pro use |
| Holes (threaded/countersunk), cosmetic threads | Y | Y | Y | Y | Y | P (libraries) | **v1** | High value; MVP can use plain cut |
| Modeled threads (for printing) | Y | P | P | P (FeatureScript) | P (1.0 hole) | P (libraries) | **v1** | Hobbyists print threads |
| Linear / circular / pattern-along-curve | Y | Y | Y | Y | Y | Y (loops) | **MVP** (linear/circular), v1 (curve) | Daily for pros |
| Mirror | Y | Y | Y | Y | Y | Y | **MVP** | Trivial and daily |
| Booleans / multi-body | Y | Y | Y | Y | Y | Y | **MVP** | Needed for print splitting, CSG users |
| Text / emboss | Y | Y | Y | Y | Y (ShapeString) | Y | **v1** | Very popular in prints |
| Parameters / variables / expressions | Y | Y (equations) | Y (variable table) | Y (variables, Variable Studio) | Y (spreadsheet, VarSet) | Y (native) | **MVP** | Core to parametric + agents |
| End-user parameter UI (Customizer) | N | N | N | P (configurations) | P | Y | **v1** | OpenSCAD's ecosystem shows demand |
| Configurations / variants | P | Y | Y (family of parts) | Y | P | P (variables) | **v1** | Pros use weekly |
| Assemblies: joints/mates | Y | Y | Y | Y | Y (1.0 Assembly wb) | N | **v1** (basic fasten/revolute/slider in MVP+1) | Pro daily; hobbyists occasional |
| Motion study / interference | Y | Y | Y | P | P | N | **later** | Pro, not core |
| 2D drawings (views, dims, BOM) | Y | Y | Y | Y (weaker) | Y (TechDraw) | N | **v1** | Pro daily; hobbyists rare |
| GD&T / MBD | P | Y | Y | P | P | N | **later** | Specialist |
| Sheet metal | Y | Y | Y | Y (weaker) | P (add-on) | N | **later** | Weekly for some pros; large scope |
| Surfacing (NURBS surfaces, boundary, patch) | Y (+T-splines) | Y | Y | P | P (Surface wb) | N | **later** | Heavy kernel work |
| Direct modeling / push-pull | Y | P (Move Face) | Y (synchronous) | P (Move face, Replace face) | P | N | **v1** (move/offset face recorded as features) | Edits imported STEP; Solid Edge's main strength |
| History-free mode | Y | N | Y | N | N | N | **later** | Rule 14 covers the need without a mode switch |
| CAM | Y (paid adv.) | P (add-in) | Y† (CAM Pro, paid) | Y† (CAM Studio) | Y (CAM wb) | N | **later** | Huge domain; export to external CAM first |
| FEA simulation | Y (paid, cloud) | Y (add-in) | Y | Y† | Y (FEM) | N | **later** | Rare for hobbyists |
| Generative design | Y (paid) | P | Y† (add-on) | N | N | N | **later** | Niche |
| Rendering | Y | Y (Visualize) | Y (KeyShot) | Y† (Render Studio) | P (Render wb) | N | **later** (good viewport PBR in v1) | Rare daily use |
| Version control / branching / merge | P (versions only) | P (PDM) | P (PDM) | Y | P (git on FCStd is weak) | Y (git on text) | **MVP** (local micro-versions + named versions), v1 (branch/merge) | Onshape's killer feature; agents need checkpoints |
| Real-time collaboration | P (cloud) | P (3DEXPERIENCE) | P | Y | N | N | **later** | Local-first first |
| Scripting / API | Y (Python/C++) | Y (VBA/C#) | Y (COM) | Y (FeatureScript, REST) | Y (Python) | Y (language itself) | **MVP** (MCP server + script API) | Project goal: agents do anything a human can |
| Code-as-model text format | N | N | N | P (FeatureScript) | P (macros) | Y | **v1** | OpenSCAD's draw, combined with B-rep |
| STEP import/export | Y | Y | Y | Y | Y | N | **MVP** | Pros' daily interchange; OpenSCAD's gap |
| IGES import/export | Y | Y | Y | Y | Y | N | **v1** | Legacy data |
| STL export | Y | Y | Y | Y | Y | Y | **MVP** | 3D printing daily |
| 3MF export (with units/colours) | Y | Y | Y | Y | Y | Y | **MVP** | Modern printing default |
| STL/3MF/OBJ mesh import (reference, mesh→solid) | Y | Y | Y | P | Y | Y (import) | **v1** | Remixing printables |
| DXF/SVG import/export | Y | Y | Y | Y | Y | Y | **v1** | Laser cutting, sketch underlay |
| Native-format import (SLDPRT, F3D, etc.) | P | P | P | Y (translators) | N | N | **later** | Proprietary, hard |
| Measure / section view | Y | Y | Y | Y | Y | N | **MVP** | Daily |
| Offline use | P | Y | Y | N | Y | Y | **MVP** | Core differentiator |
| Free + private + commercial use | N | N | N (Community: non-commercial) | N | Y | Y | **MVP** | Our open-source licence gives this |
| Cross-platform (Win/Mac/Linux) | P (no Linux) | N | N | Y (browser) | Y | Y | **MVP** | SW/SE Windows-only pain |

---

## 6. Injection attempts observed

None observed. No fetched snippet contained instructions aimed at the agent. (Direct page fetches all failed, so
exposure was limited to search-result summaries.)

## 7. Gaps / follow-ups

- Re-run the Reddit cluster when access is possible (via the official API or a human export). The frequency labels for the
  r/3Dprinting and r/MechanicalEngineering segments are inferred from aggregators.
- Verify the matrix cells against the vendors' current docs in the feature-planning ADR, especially Onshape plan pricing (sources
  disagree: $2,100 vs $2,500 for Professional) and Fusion personal-tier terms in 2026.
- A small user survey (Printables/Discourse) on daily feature use would replace the inferred table in section 3.
