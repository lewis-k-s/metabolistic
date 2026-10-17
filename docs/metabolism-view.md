# Metabolism view (Manage screen redesign)

Status: design, 2026-10-16. Branch `core-gameplay-design`, after M8.

## Problem

The Manage screen is a list of text panels (genome, enzymes, membrane,
queue, action slots). A player cannot see how the cell uses the resources it
takes in: which pathway eats which pool, which output goes where, and why a
pool goes down. The art for the membrane proteins exists, but the screen does
not use it.

## Goal

The Manage screen is a **cross-section of the cell**:

- **Top: the membrane.** A close-up of the lipid bilayer. Each membrane slot
  holds its protein sprite, embedded in the bilayer and pointing out. The
  outside (vent heat, minerals, light) is above the bilayer.
- **Middle: the cytoplasm.** A metabolic map. Pools are molecule nodes.
  Pathways are plates with their enzymes on them. Pipes join them, and
  molecules flow along the pipes at the live rate.
- **Bottom: the ribosome.** It builds the queue. A finished membrane protein
  flies up to its slot; a finished enzyme flies to its pathway.

The player must see, at a glance:

1. what comes in (through the membrane, or as an outside energy such as heat
   or light);
2. what each pathway turns into what, and how fast;
3. what limits each pathway (the input that runs out, the missing heat, no
   enzymes);
4. where each output goes (proteins, repair, upkeep, waste).

## Decisions (2026-10-16)

| Question | Decision |
|---|---|
| Run view while the editor is open | Full-screen cross-section, plus a **small live inset of the run** in a corner. The inset edge flashes red when the cell takes damage. Time still runs outside a shelter. |
| Which pathways the map shows | **Fixed map with ghosts.** Each pathway has a fixed place, like a textbook map. A pathway gene that the cell does not have is a dim outline. Its name shows only if the codex has the gene; otherwise it shows "?". |
| How to edit | **Click a node, side panel.** A click on a pathway or a slot opens a side panel: +/- (queue, recycle), the level buttons Off / Low / High (pathways), rates in and out, the limit. Hover shows a short tooltip. |
| Rendering | World-space pixel sprites on their own render layer and camera. Bevy UI only for the header, the side panel, tooltips and the action bar. |
| Layout | Hand-placed, in `layout.rs`. Not computed from the graph. |

## Visual language

Follows `art-direction.md` (top-down pixel art, one palette, 1 px outline,
colour means role, dithered glow).

| Thing | Look | What changes it |
|---|---|---|
| Pool | the molecule icon of the currency, with a short level gauge | Level (log scale). Flashes red below a low mark. Number in the tooltip. |
| Pathway | a plate with the pathway icon | Gene state: Silent = grey, Mutated = cracked plate, Enhanced = gold rim. Ghost = dithered outline. |
| Enzyme | small enzyme sprites on the plate, one for each enzyme (more than 6: one sprite with "xN") | They bob at a speed that follows the pathway activity. |
| Throttle | a valve icon on the plate (Off / Low / High) | The pathway level. A click on the valve moves it to the next level (1 ATP). |
| Pipe | a dotted pixel line from pool to pathway, and from pathway to pool | Width tier (1-3) from the flux. |
| Flow | molecule specks (the pool icon, small) that move along the pipe | Spawn rate follows the flux (log scale). No flux, no specks. |
| Cofactor | a dashed pin from the pool to the plate, no specks | The cofactor is needed but not used up. |
| Outside energy | heat (orange-red) or light (pale yellow) waves that come down through the bilayer to the pathway | The grid sample at the cell (the same colours as the Run grid). |
| Limit | the limiting pipe pulses red and the plate shows a "!" badge | `block_limit` (see Data). |
| Other flows | short stubs on a pool with a small label icon: upkeep, movement, actions, membrane repair, genome repair, food, engulf | `FlowLedger` (see Data). |

### Membrane band

- The bilayer is `wall_membrane` tiled sideways (it is a bilayer
  cross-section and tiles sideways already). Membrane integrity shows in its
  colour and cracks (art rule 6).
- The band has `membrane_slots(size)` sockets, spaced evenly.
- A used slot shows the overlay sheet, row `N` (pointing out), `idle`. It plays
  `use` (or `hold`) while its action is on, from `ActionState`.
- An empty slot is an empty socket.
- A queued membrane protein is a dithered ghost of its overlay in the next free
  slot, with the build progress.
- A transporter in `use` streams molecules down into the Minerals pool pipe,
  so the membrane and the map join visibly.
- If the slots do not fit at `PIXEL_SCALE`, the band uses scale 2.

Protein to sheet (one shared function, the Run screen can use it too):

| `MembraneProtein` | Sheet |
|---|---|
| Transporter | `overlay_transporter_mineral` (until a transporter has a substrate) |
| Flagellum | `overlay_flagellum` |
| Adhesin | `overlay_adhesin` |
| Toxin | `overlay_toxin` |
| Lytic | `overlay_lytic` |
| Receptor | `overlay_receptor` |

### Map layout

Lanes from top to bottom. The flow goes down, except waste and oxygen, which
have their own column on the right and go back up and out.

| Lane | Contents |
|---|---|
| 1. Intake | Minerals, Metals (cofactor), O2 in. Heat and light come in from the membrane. |
| 2. Energy pathways | Chemosynthesis, LightCapture, Respiration, Fermentation |
| 3. Hub pools | ATP, ReducingPower, CarbonSkeletons, Pyruvate |
| 4. Build pathways | AminoAcidBiosynthesis, LipidMetabolism, NucleotideCofactorSynthesis, SecondaryMetabolites |
| 5. Products | AminoAcids, Lipids, Nucleotides, Pigments |
| 6. Users | ProteinSynthesis (the ribosome), membrane repair, genome repair, UV screen, Cuticle, FatStorage |
| Right column | Waste -> VesicleExport -> out; ROS -> Catalase -> O2 -> out |

Only genes with a `block_spec` get a place. Genes with no block
(`SugarCatabolism`, `OrganicAcidOxidation`, `NitrogenSulfurAssimilation`,
`AromaticPrecursorSynthesis`, `Polymerization`, `Multicellularity`) are not on
the map.

## Data (headless, pure)

New code, all in the simulation modules, with no dependency on `screens`:

- **`metabolism::solver::block_limit(pools, block) -> Limit`.** The reason a
  block runs below its full rate: `Off` (drive 0 or Silent), `NoEnzymes`,
  `External(Channel)`, `Input(Currency)`, `Cofactor(Currency)`, or
  `Shared(Currency)` (the pool is split with other blocks). It uses the same
  terms as `block_demand` and `block_rates`, so it agrees with the solver.
- **`FlowLedger` component on each cell.** Every pool change outside the
  solver is recorded with a source (`Upkeep`, `Movement`, `Action`,
  `MembraneRepair`, `GenomeRepair`, `Intake`, `Food`, `Engulf`, `Recycle`,
  `Export`). Writers today: `cell`, `exchange`, `actions`, `membrane`,
  `genome_damage`, `particles`, `engulf`, `walls`, `proteins`, `oxygen`. The
  ledger keeps a moving average (about 1 s) per source and currency, so the
  view does not flicker.
- **`metabolism::view::MetabolismView`.** A pure function builds it from the
  genome, the codex, the proteome, each block (kind, profile, turnover, status,
  throttle, kinetics), the pools and the ledger. It has the pools (level), the
  pathways (state, enzymes, activity, limit, ghost or owned), and the edges
  (from, to, currency, flux per second, kind: input, output, cofactor, outside,
  other). Flux per second = smoothed `BlockTurnover x profile / dt`.

Tests: the view's flux matches the pool deltas of a tick; `block_limit` names
the starved input in a set of hand-made cases; the ledger sums match the pool
changes outside the solver.

## Screen code (full only)

`src/screens/manage/`:

- `mod.rs`: state, input, plugin. `ManageAction` and the queue/recycle
  messages stay as they are.
- `layout.rs`: the fixed map. A pure table of positions for each `BlockKind`
  and `Currency`, and the pipe routes. Tests: every block with a spec and every
  currency in a profile has a place; no two nodes overlap.
- `diagram.rs`: spawns the map sprites once, then updates them from
  `MetabolismView` each frame (no rebuild, so hover and animation stay smooth).
- `membrane_view.rs`: the bilayer band and the slot sprites.
- `panel.rs`: the side panel and the tooltips (Bevy UI).
- `inset.rs`: the run camera draws into a corner viewport while Manage is
  open, above the cell camera. The run HUD is not drawn in the inset.

Picking: sprite picking (`Pointer<Click>`, `Pointer<Over>`) on the plates and
the sockets. Check the Bevy 0.19 sprite picking backend with context7 first.

The old text panels go. The action bar stays as a UI row at the bottom.

## Art to add (`art/gen_sprites.py`)

| Sprite | Notes |
|---|---|
| `mol_<currency>` (13) | 10x10 icons. Reuse the shapes of `food_carbon`, `pickup_mineral` and `pickup_metal_fes` for CarbonSkeletons, Minerals and Metals. The other ten need a colour in `RAMPS`; gold-white stays for the gene reward only. |
| `enzyme` | 16x16, 8-frame idle bob, protein blue. A pathway accent tint at run time. |
| `pathway_plate` | a frame that stretches (nine-slice), plus a ghost (dithered) look |
| `pathway_icon_<block>` | 16x16, one for each block with a spec |
| `ribosome` | 24x24, two subunits, 8-frame loop |
| `slot_socket` | the empty socket on the bilayer |
| `badge_limit`, `valve_off/low/high` | small UI icons |

Until the art exists, the screen uses tinted stand-ins, so the code does not
wait for the art.

## Work plan

| Step | What | Depends on |
|---|---|---|
| A | Data: `block_limit`, `FlowLedger`, `MetabolismView`, tests (`cargo th`) | - |
| B | Art: the sprites above, manifest, contact sheet | - |
| C | Screen: layout, map, membrane band, inset camera, with stand-in art | A |
| D | Interaction: picking, side panel, tooltips, remove the old panels, smoke test | C |
| E | Swap the stand-ins for the art of B | B, C |

A and B can run at the same time. C and D can be one session.

## Open points

- Nothing makes Pyruvate, so Fermentation can never run. The map will show
  this as a pool with no input pipe. Decide whether `SugarCatabolism` gets a
  block, or Fermentation takes another input.
- The Run screen still mounts one placeholder flagellum. With the shared
  protein-to-sheet function, it can mount the real proteome.
