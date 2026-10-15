# Gameplay core: design and roadmap

Status: agreed in a design review on 2026-10-15. M1 and M2 are done and
merged to `master` (2026-10-15). M3 and M4 are done. M5 is done. M6 is done (not committed yet). Next: tune the vent slice, then M7.
This document records the decisions and the order of work. Items marked
**playtest** need numbers from play before we fix them. The art rules are in
`docs/art-direction.md`.

## 1. The game in one paragraph

You are one cell, a blob, in a 2D sprite world. Each level is an
evolutionary stage: deep-sea vents, then shallow water, then land. You move
through a field of resources and hazards that you can see: heat, minerals,
light, oxygen, water. Your metabolism turns what you absorb into energy and
into proteins. Enzymes make your pathways faster. Membrane proteins give you
actions: move, absorb, attack, pass through walls. To finish a level, you
must engulf its gate cell and take its gene. That gene is the innovation that
lets you survive the next stage. You die when ATP or membrane integrity
reaches zero.

## 2. Decisions

### Run and death

- Death: ATP = 0, or membrane integrity = 0.
- Advance: you must have the stage's key innovation. A missing innovation is
  also a hazard. Example: early light zones are deadly because nothing
  protects your genome from UV.
- Each level ends with a gate cell. When you engulf it, you take its gene and
  win the level.

### Proteins and enzymes

- Amino acids + ATP make protein units. You allocate them:
  - to a pathway, as **enzymes**: they make the pathway faster;
  - to the membrane, as **transmembrane proteins**: transporters, flagella,
    adhesins, toxins, receptors.
- Proteins do not decay. You can recycle a protein for about half of its
  amino acids.
- Limits:
  - Membrane proteins need surface area. A bigger cell has more area, but it
    is slower and costs more ATP to maintain.
  - Pathways compete for the same primary inputs, so you cannot run many at
    full rate. Stacking enzymes in one pathway is a valid min-max. Each stage
    still needs some variety to survive its resources and hazards.
- Actions cost ATP and/or a cofactor for each use.

### Cofactors

- Start with one: **Metals** (Fe-S clusters), from the vents.
- Each later stage adds one at most. Fewer than four in total.

### Mutation and UV

- UV lowers membrane integrity at once (lipid peroxidation), and it causes
  gene mutations at a rate.
- A mutated gene gives a weaker block. Repair costs ATP + Nucleotides.
- Moderate UV lowers transcription/translation efficiency: protein synthesis
  is slower.
- Extreme UV fragments the genome: you cannot make new proteins.
- Pigments (`SecondaryMetabolites`) screen UV.
- Optional high-risk strategy: induce mutation on purpose for a chance of a
  beneficial effect.

### New genes

- Main source: horizontal gene transfer. You engulf or merge with a cell and
  take one of its genes.
- Gate cell at the end of each level: a guaranteed, stage-specific gene.
- Viruses: a risky gene transfer. You get a gene or a mutation.
- Beneficial mutation: rare, and only from induced mutation.

### Editing and live control

- You can open the editor at any time. Game time keeps running, unless you
  are in a **shelter**.
- Shelters: a vent crevice, a microbial mat, a moist soil pore. Each level
  has only a few. Inside a shelter, hazard fields are calm and hostile cells
  do not enter. A shelter is not a save point. **Playtest:** whether to slow
  time outside shelters, and if a shelter has limited resources.
- Structure (genes, enzyme and membrane allocation) is slow, like protein
  synthesis. The editor makes a synthesis queue, and your metabolism builds it
  in game time.
- Regulation is fast, like gene expression. About **4 action slots**. A slot
  holds a pathway (Off / Low / High) or a membrane protein action (flagellum
  burst, toxin, adhesin). Each change has a short ramp time and a small ATP
  cost. Passive effects of pathways always apply; only actions use slots.

### Pathway competition (solver change)

- Pathway rate = enzymes x S / (K + S), where S is the input substrate. This
  is a simplified Michaelis-Menten rate. Enzymes scale the rate linearly. The
  substrate supply sets the ceiling.
- When the total demand for a pool is more than the pool holds, each consumer
  gets a share in proportion to its demand.
- This replaces the current greedy order in `solve_tick`, where the first
  block in `BlockKind` order takes all of a shared input. Stoichiometry stays
  conserved, pools never go negative, and the result must not depend on the
  tick length.

### Spatial environment

- A coarse grid field per external channel: Heat, Minerals, Light/UV,
  OrganicCarbon, O2, Water (land only). Each channel has cheap diffusion,
  sources and decay.
- A cell samples the grid at its position. Transporters set how fast
  substances move between the grid and the cell's pools.
- Visual styling shows each channel (colour, glow, particles), so the player
  sees where to go and where to flee.
- This replaces the global `EnvironmentalCurrencyPools` resource.

### Internal pools (currency set)

| Group | Pools |
|---|---|
| Energy | ATP, ReducingPower |
| Carbon hub | Pyruvate, CarbonSkeletons (absorbs AcetylCoA) |
| Building | AminoAcids (proteins), Nucleotides (genome repair), Lipids (membrane repair and growth; absorbs FreeFattyAcids and StorageBeads as a stored form) |
| Cofactor | Metals |
| Taken in | Minerals (filled by transporters from the grid; used by chemosynthesis) |
| Waste | Waste |

Membrane integrity costs Lipids to repair. This connects defence to
metabolism.

### World membranes and tissue

- A membrane wall blocks movement and also blocks diffusion in the grid.
- Three ways to deal with a wall:
  - a matching **transporter** pulls substrate through it (feed without
    passing);
  - a **fusion/adhesin** protein lets you pass through slowly;
  - a **lytic** enzyme damages it. It costs a cofactor, and some walls fight
    back.
- Tissue is cells joined by walls. The number and range of tissue types
  increase with each stage.

### Other entities

- Prey, predators, symbionts and gate cells use the same `Cell` model as the
  player, with a real `Genome`. When you engulf one, you can take its genes.
- They run a slower metabolic tick than the player. Their behaviour comes
  from a small rule set for each archetype.
- Food particles and viruses are light entities with no metabolism.
- **Playtest:** the CPU cost of NPC metabolism. Simplify NPC behaviour or
  metabolism if necessary.

### Movement

- avian2d. Circle colliders with linear drag.
- Thrust comes from flagella or cilia. Size sets mass and drag, so a bigger
  cell is slower.
- The blob look is visual only. The body deform is baked into 8-direction
  animation frames (see `docs/art-direction.md`). Do not scale or rotate a
  cell sprite at run time: rotation turns the light. There is no soft-body
  physics.
- Land uses the same controls and model, with a different drag value.

### Meta progression

- A codex: each gene that you obtained once can be a starting gene later.
- At the start of a run, you choose a small starting genome from the codex,
  with a budget.
- No stat upgrades that carry over between runs.

### Stages

| Stage | Primary energy | Main hazard | Key innovation | Gate cell gives |
|---|---|---|---|---|
| Vents | Chemosynthesis: Minerals + Heat -> ATP, carbon | Heat at the vent core; scarcity away from it; deadly light zones | Pigments (UV screen) | LightCapture (a pigmented phototroph) |
| Shallow water | Photosynthesis: Light -> ATP, ReducingPower | UV; **your own O2 and reactive oxygen species** | O2 detox (catalase), then respiration | Respiration (an alpha-proteobacterium, as a mitochondrion) |
| Land | Respiration: O2 + carbon -> much more ATP | Desiccation (Water field) | Cuticle / thick membrane | Open (multicellularity?) |

The shallow-water stage is the Great Oxidation Event. Your new, more powerful
metabolism is the source of the risk: the pathways you build produce reactive
oxygen species that damage you. The tone is a little body horror. On land,
you must stay near water, the world you left behind.

## 3. Current code compared with the design

Updated after M6.

| Area | Now | Still to do |
|---|---|---|
| Solver | Enzymes x saturation, proportional sharing, fixed 0.05 s sub-steps (`metabolism/solver.rs`) | - |
| Pools | The pools above (with Pigments), plus a `StoredLipids` component | - |
| Blocks | Fermentation, FatStorage, VesicleExport, Chemosynthesis, ProteinSynthesis, AminoAcidBiosynthesis, LipidMetabolism, NucleotideCofactorSynthesis, SecondaryMetabolites | photosynthesis, catalase, respiration (M7, M8) |
| Proteins | `Proteome`, `SynthesisQueue`, recycling, membrane slots from size, editor UI | - |
| Genome | `Silent` / `Expressed` / `Mutated` / `Enhanced`; UV mutation load, repair, translation efficiency, `GenomeIntegrity`; gene transfer by engulf and virus | Mutation from ROS |
| Environment | `EnvGrid` with Heat, Minerals and Light: diffusion, drift, sources, decay | OrganicCarbon, O2, Water |
| Run screen | The player is a `Cell` with a body; the level comes from the stage's `LevelDef`; HUD with stage name and seed; run-over screen with the cause of death or "Run complete", and the new codex genes | - |
| NPCs and world | NPC `Cell`s on a slow tick (every 4th), steering, engulf, symbiont exchange, food, metals, viruses, toxin hits, walls with pores, lysis, gate cell | Fusion protein, immune proteins |
| Physics | avian2d, 64 Hz, no interpolation | - |
| Art | Baked 8-direction sheets, integer scale (3x), dithered grid texture | A final light colour; a sprite for the shelter disc |
| Manage screen | Editor: genome, enzymes, membrane slots, queue with time to finish, action slots | - |
| Run structure | `RunState` (seed, stage, genes taken, carry), `Stage` + `LevelDef`, Prepare screen with a codex budget, Stage complete screen, `Codex` saved as JSON, seeded vent layout with a validator | Levels for shallow water and land (M7, M8) |

Out of date: the multiplayer and mitochondrial-genome items in `TODO.md`, and
`flowmap_design.md`. Mitochondria now come from endosymbiosis (the shallow
water gate cell), not from a second genome system.

## 4. Roadmap

Each milestone ends with something that runs and that we can test. Milestones
1 and 3 are headless work (`cargo th`). The others need the smoke test.

```mermaid
flowchart LR
  M1[M1 Sim core] --> M2[M2 Cell in a field]
  M1 --> M3[M3 Genome damage]
  M2 --> M3
  M2 --> M4[M4 Build and regulate]
  M3 --> M4
  M4 --> M5[M5 Living world]
  M5 --> S1([Vent slice])
  S1 --> M6[M6 Run structure]
  M6 --> M7[M7 Shallow water]
  M7 --> M8[M8 Land]
```

### M1. Simulation core (headless) - done, 2f79b3f

Notes: chemosynthesis, amino acid and lipid synthesis were all needed for a
vent cell to live. Cell maintenance costs ATP in proportion to size. Pools
have no upper limit, so a fed cell builds a big surplus (**playtest**).

1. Change the pool set to the 9 pools. Move Fermentation, FatStorage and
   VesicleExport to it.
2. Change `solve_tick`: enzyme count per block, saturation, and proportional
   sharing of shared inputs. Proptests: mass conserved, no negative pools,
   tick-length independence, more enzymes never lowers a block's rate.
3. Protein economy: a protein synthesis block (AminoAcids + ATP -> protein
   units), an enzyme allocation for each block, membrane slots from cell size,
   a synthesis queue, recycling.
4. Chemosynthesis block (Minerals + Heat -> ATP, carbon), with Metals as a
   cofactor.
5. Membrane integrity on the cell; repair costs Lipids.

Done when: a headless test cell lives on a constant mineral supply, builds
enzymes from a queue, and two pathways share an input in proportion to their
enzymes.

### M2. A cell in a field (first playable) - done, 9fcb708 and c6f9789

Notes:
- Minerals are an internal pool. Transporters fill it from the grid (slow
  with none), and the solver limits chemosynthesis by it. Heat is a
  condition, not used up. Heat above a threshold damages the membrane.
- The run-over screen shows the cause: starved, or membrane failed and the
  main damage source (`DamageTaken`).
- World units are sprite pixels; the camera draws them at 3x. Player radius
  is 20. Grid cell is 32 world units.
- **Playtest:** the plume has far more minerals than a cell can take, so
  depletion does not matter yet. A starving cell keeps a tiny ATP trickle, so
  ATP at or below 0.01 counts as 0.

1. `EnvGrid` resource: channels, diffusion, sources, decay as pure functions.
   Replace `EnvironmentalCurrencyPools`. Transporters exchange with the grid
   cell under the body.
2. The Run player is the local player's `Cell`, with a body. Replace the
   separate `RunPlayer` disc.
3. avian2d: circle collider, drag, thrust from flagella, size -> mass and
   drag. Movement costs ATP.
4. Sprite rendering of the grid channels (heat glow, mineral plumes, light).
   The blob wobbles with velocity.
5. Death on ATP = 0 or membrane = 0, and a run-over screen.
6. HUD: ATP, membrane, main pools.

Done when: you can play a vent field, find minerals, and starve or burn.

### M3. Genome damage - done (not committed yet)

Notes:
- `Channel::Light` in `EnvGrid`. UV = light x `UvFraction` (1.0 in the
  vents). Two shafts in the vent field (`level::SHAFTS`): A between the start
  and the second vent, B east of it. The player goes under A and over B.
  Core level is about 9-10, the edge fades over about one grid cell.
- UV damage, both x (1 - shield): membrane 0.8 per UV per second (`DamageTaken`
  as UV), genome integrity 0.008 per UV per second, mutation load 0.04 per UV
  per second. Unshielded, a cell dies in the core in about 15 s.
- Translation efficiency: 0 below integrity 0.25, then 0.2 rising to 1. It
  multiplies the `Drive` of protein synthesis.
- Mutation: `MutationLoad` accumulator, seeded `GeneRng`. The old random
  mutation system and `MutationConfig` are removed. The game seeds the RNG from
  the system, `new_headless` uses a fixed seed. Mutation runs in
  `MetabolicSet::Genome`, so a mutation shows one tick after the exposure.
- Repair: 0.02 work per second (work 1 = integrity 0 to 1). 60 ATP and 30
  Nucleotides per work. A mutated gene needs 0.1 work. Work splits half and
  half between integrity and genes. It stops at the gap and when a pool is empty.
- `NucleotideCofactorSynthesis` (in the starter genome and proteome) and
  `SecondaryMetabolites` (pigments) are regulated refill blocks (`blocks::refill`):
  they stop at a target (20 Nucleotides, 40 Pigments) and never overshoot.
  Pigments decay 0.01 per second. Shield = P / (10 + P), capped at 0.999.
- Gene orb at `level::GENE_ORB_POSITION`, in the dim edge at the lower end of
  shaft A. It gives the pigment gene with one enzyme. It is a stand-in for gene
  transfer (M5).
- HUD: Genome bar, UV and shield line. Run-over names UV.

Original plan:

1. A Light/UV channel: light shafts from above in the vent level. UV damages
   the membrane and lowers a `GenomeIntegrity` value.
2. Replace random mutation with a mutation load from UV. Each time the load
   passes 1, one expressed gene becomes `Mutated` (seeded RNG).
3. Repair (ATP + Nucleotides). Translation efficiency lowers synthesis rate.
   Extreme damage fragments the genome. A nucleotide synthesis block, because
   nothing makes Nucleotides yet.
4. Pigment block screens UV: a `Pigments` pool that decays slowly; shield =
   P / (K + P).
5. A `gene_orb` pickup gives the pigment gene. It stands in for gene transfer
   until M5.

Done when: light zones kill a cell without pigment, and a cell with pigment
survives.

### M4. Build and regulate - done (not committed yet)

Notes:
- Time rule in one pure function, `shelter::sim_paused(in_shelter, editor_open)`:
  stop only in a shelter with Manage open. It pauses `Time<Virtual>`, so the
  metabolic tick, the grid and physics all stop. Outside a shelter, full speed.
  **Playtest** (maybe slow time instead).
- Shelters: `level::SHELTERS`, two circles of radius 48: (120, -130) near the
  start and (480, 380) near vent 2 / the far plume. Heat and UV damage are 0
  inside (`InShelter`). Resources are unlimited. The disc is a generated dark
  translucent sprite with a lighter rim (water colours): **art owner** may
  replace it.
- Slots (`actions.rs`): Off 0 / Low 0.5 / High 1.0, ramp 1.0 per second capped
  at the gap, 1 ATP per change (refused with less). The level is a `Throttle`
  on the block that multiplies `Drive`, not `Drive` itself, because regulated
  blocks (protein synthesis, fat storage, refill) rewrite `Drive` every tick.
- Flagellum burst: thrust x3 for 1.5 s, 3 ATP, 4 s cooldown that starts when
  the burst ends. Movement ATP also rises x3 (it follows thrust). Adhesin:
  toggle, thrust off, damping 60, 0.2 ATP/s, lets go at 0 ATP. Toxin: droplet
  (240 px/s, 1.2 s, fades), 2 ATP + 1 Metals. Metals has no source yet, so the
  starting 5 limit the shots (and low Metals slow chemosynthesis, its cofactor).
- Induced mutation: 10 ATP, +1.0 `MutationLoad` and +1 `InducedMutations`.
  When the unit turns into a mutation, 15% (`ENHANCE_CHANCE`, `GeneRng`) become
  `Enhanced` (x1.25; repair of integrity leaves it, but a later mutation, UV or induced, turns it to `Mutated`, and repair then gives `Expressed`: the bonus is lost). UV mutations never enhance.
- A pathway that is not in any slot runs at High, so a level cannot get stuck
  at Off after the slot changes (`effective_level`).
- Starter bar: slot 1 Chemosynthesis, slot 2 Induce mutation. Hotkeys 1-4 on
  Run; in Manage they re-assign the slot. Overlays: flagellum, adhesin, toxin,
  mineral transporter (idle only), max 2 of a kind and 6 in all. The flagellum
  points away from the aim.
- Editor ETA uses the last tick's synthesis rate, or the ribosome estimate
  when the queue was empty.

Original plan:

1. Manage screen: genome, enzyme allocation, membrane slots, synthesis queue.
2. Shelters: calm zones where game time stops while the editor is open.
3. Action bar with 4 slots: pathway Off / Low / High, and membrane protein
   actions, with ramp time and ATP cost.
4. Hotkeys for the slots.
5. Induced mutation as an action (risky). Moved from M3, because it needs a
   slot.
6. Membrane protein overlays on the player (art exists).

Done when: you can plan a build in a shelter, see it get built in game time,
and switch pathways during play. **Playtest** the time rules here.

### M5. Living world (vent slice) - done (not committed yet)

Notes:
- **Slow tick.** `CellTick { every, phase, due }` on every cell. NPCs run every
  4th metabolic tick with 4 x dt. Every per-cell system reads `due` and the
  cell's dt. A test shows the same pools after 16 normal ticks and 4 slow ones.
- **NPCs** (`npc.rs`): prey (radius 11.25, speed 85, sense 160), predator
  (radius 25 = 1.25 x player, one flagellum, speed 85, sense 230), symbiont
  (radius 9.5, speed 95, sense 400), gate (radius 25, static). Steering is a pure
  function (`steer`) at about 7 Hz; prey and predators also leave heat above 25
  and light above 0.4. Genomes have chemosynthesis plus one signature gene: prey
  Fermentation (the three prey at shaft A: SecondaryMetabolites), predator
  VesicleExport, symbiont FatStorage, gate LightCapture.
- **Engulf** (`engulf.rs`): contact (gap <= 3 px) + radius >= 1.2 x target +
  intent. The player holds **Space**; predators always intend. 50% of ATP,
  carbon, amino acids; one gene (seeded) that is absent, silent or mutated in the
  engulfer, added Expressed with one enzyme **queued** (not free). A predator
  engulfing the player ends the run: "Engulfed". The gene orb is removed.
- **Symbiont**: in contact (gap <= 10) carbon goes to the player and ATP to the
  symbiont, each side at 0.1 per second towards balance (exact, no overshoot).
  It starts with 60 carbon and 40 ATP.
- **Particles** (`particles.rs`): food 6 carbon, cap 40, one per 4 s; metals 1,
  cap 14, one per 6 s near the vents (this is the Metals source); dead NPCs drop 8
  carbon. Virus: drifts at 18 px/s; 50% gene from {Fermentation, FatStorage,
  VesicleExport} (queued), 50% +1.0 mutation load. Toxin hit = 25% of max
  membrane (the brief said "0.25 integrity"; integrity is 0-100 here), so 4 hits
  kill a prey.
- **Walls** (`walls.rs`): tiles = grid cells. `EnvGrid` has a wall mask (inert
  cells: no source, no flow in or out; mass conserved). A pore is solid for a
  cell without a Transporter (physics layers). Adhesin held on a wall: the cell
  ghosts through at 25 px/s. New `MembraneProtein::Lytic` and action Lyse (5
  ATP, cuts the tile in contact; the tile stays gone). Pores also block the grid.
  Settled light does not change when a wall is cut. **Fusion: out of scope.**
- **Gate** (`gate.rs`): 3 s of contact wins. If contact breaks for more than
  0.25 s the progress **resets** to 0. Gene LightCapture (placeholder, no block),
  victory clip 1.6 s, run-over shows "Victory" and the gene.
- **Level**: gate chamber (x 800-960, y 0-192) with two pores in its south wall
  and its own light zone; a wall line at x 576-608 from shaft B up to the top
  (pores at y 384 and 544); a short line south of the core (y -256). 15 creatures,
  5 viruses, food and metal zones. Vertical wall pieces are the horizontal sprite
  turned 90 degrees (art owner: draw vertical pieces).
- **CPU** (headless, `tests/npc_benchmark.rs`, ignored): per metabolic tick
  (averaged over the 4-tick cycle, includes the grid step): 20 NPCs 0.25 ms, 50
  NPCs 0.30-0.41 ms, 100 NPCs 0.43-0.54 ms. One frame of behaviour: 0.14, 0.15,
  0.15-0.23 ms. Cost per game frame (5 ticks per second): 0.16 / 0.17-0.19 /
  0.19-0.28 ms. Far below 2 ms, so no simplification was needed.
- **Respawn** (`npc.rs`): empty level slots refill one at a time every 25 s (never
  above the starting count, never within 260 px of the player). While the player
  has no Expressed or Enhanced pigment gene, the 3 pigment prey are topped up
  every 6 s. This keeps the run winnable.
- **Shelters are safe from hostile cells**: predators turn away from a shelter
  (radius + body + 30 px) and ignore prey inside one; `plan_engulfs` skips a
  sheltered target unless the engulfer is the player.
- Smoke test: the player is `Invulnerable`, a new step shows the gate chamber.
- Not done: failed-engulf and contact/hit clips, overlays on NPCs, predator
  engulf animation (the run just ends), codex of taken genes.

Original plan:

1. NPC cells with the `Cell` model at a slower tick, and simple archetype
   behaviour: prey, predator, symbiont. Measure the CPU cost.
2. Engulf: contact + size check. Gene transfer from engulfed cells.
3. Food particles and viruses.
4. Membrane walls in the world: block movement and diffusion. Transporter,
   fusion/adhesin and lytic interactions.
5. Vent level layout: vent core, mineral plumes, light zones, shelters,
   tissue, the gate cell.
6. Gate cell: engulf it to win the level.

Done when: a full vent run can be won or lost. This is the **vertical
slice**. Tune it before we build more stages.

### M6. Run structure - done (not committed yet)

Notes:
- **Stage data.** `LevelDef` (`level.rs`) holds everything a level needs: grid
  size and field physics (`FieldKind`), vents, shafts, UV fraction, shelters,
  chamber, wall lines with pores, NPC slots, viruses, food and metal zones, gate
  position and gene, pigment prey centre, start. The old constants stay as the
  base layout (`LevelDef::vents_base()`), so M2-M5 tests did not change.
  `Stage` (`stage.rs`) is Vents, ShallowWater, Land; only `Vents` has a level
  (`Stage::level_def(run_seed)`). `CurrentLevel(LevelDef)` is a resource while a
  stage runs; `gate_system` reads the gate gene from it. To add a stage (M7, M8):
  return a `LevelDef` for it, and add a `FieldKind` if its physics differ.
- **Run state** (`run.rs`). `RunState { active, seed, stage_index, loadout,
  genes_taken, carry, new_genes }`. Winning a stage (after the victory clip):
  the genome (mutated genes repaired), proteome and action bar are copied
  (`Carry`); if the next stage has a level, the game goes to the new state
  `GameState::StageComplete` ("Stage complete", gene taken, Continue). `InRun` is
  left and entered again, so the world is rebuilt from the new `LevelDef`. Pools
  are the starter values, membrane and genome integrity are full. If there is
  no next level, `RunSummary::run_complete` is set and `RunOver` shows "Run
  complete" and "(More stages coming.)". Entering `InRun` with no run begun (the
  Manage button, the smoke test) begins a default run.
- **Codex** (`codex.rs`). `Codex` (set of `BlockKind`) in `codex.json`. Directory:
  `METABOLISTIC_SAVE_DIR`, else the user data dir (computed, no new crate).
  Genes are tracked by message: `EngulfEvent` (player engulfs), new
  `VirusInfection`, and `GateWon` (now has the gene). They go into the codex on
  `OnEnter(RunOver)` and when the player leaves a run for the menu. A broken file
  gives an empty codex and a warning (the next save overwrites it). The headless
  app has `CodexStore { path: None }` (memory only); tests that save use a temp
  dir. The smoke test saves in `target/smoke/save`.
- **Prepare** (`prepare.rs`, `screens/prepare.rs`). Menu "Start" goes to Prepare;
  "New run" on the run-over screen too. Budget 2 points; a gene with a block costs
  1; `LightCapture` (no block) costs 0, is listed in grey and cannot be chosen;
  starter genes cannot be chosen. A chosen gene starts Expressed with one enzyme.
  The seed comes from the system RNG when Start is pressed.
- **Layout** (`layout.rs`). `generate_vents(seed)` jitters: second vent (+-30,
  +-50) and what belongs to it, start (+-12), shafts, shelters, creatures (+-35,
  pushed out of vents), viruses, zones; picks 2 pores per wall line and for the
  chamber; the pigment prey follow shaft A. `validate` is pure and checks the
  hard rules listed in the module doc. A failed layout is retried (seed, attempt
  + 1) up to 8 times, then the base layout is used. Over seeds 0-199 the first
  layout fails for 2 seeds (1%); a test allows 10 and none fall back to the base.
  Food and metal zones are circles, so a spawned particle can still land on a
  wall tile (as before M6).
- **HUD / screens**: the HUD has a first line "Stage 1/3: Deep-sea vents seed N";
  the run-over screen shows stage, seed and "New in the codex: ...". The smoke
  test visits Menu, Settings, Prepare, Run, Manage, Run, gate view, StageComplete,
  RunOver, Menu.
- Not done: more than one real stage (so the Stage-complete screen is only
  visited by the smoke test, and by a unit test of `complete_stage` with
  a fake "next stage is playable"); the seed is not shown on Prepare and the
  player cannot type one.

Original plan:

1. Run state: the stage sequence, level transitions, win and loss.
2. Codex: save obtained genes. Choose a starting genome with a budget.
3. Some level variation (procedural layout of fixed parts).

### M7. Shallow water

1. Photosynthesis block. It produces O2 into the cell and the grid.
2. Reactive oxygen species: damage from your own O2. Catalase block.
3. Respiration gate cell (endosymbiosis).
4. More tissue types.

### M8. Land

1. Respiration block (O2 + carbon -> much more ATP).
2. Water channel and desiccation. Cuticle / thick membrane.
3. Land drag value. Tissue at its most complex.

## 5. Open questions

- Time rules for the editor outside shelters (stop, slow, or full speed).
- Shelter resources: unlimited or limited.
- NPC metabolism cost, and how much to simplify it.
- The second and third cofactors.
- What the land gate cell gives (multicellularity?), and if there is a
  fourth stage.
- Grid resolution compared with cell size (now: grid cell 32, player radius
  20).
- Mineral scarcity: should plumes run dry under a cell?
- The art includes immune proteins (MHC I and II, TCR, antibody, perforin).
  They are not in this roadmap yet: which stage or mechanic uses them?
