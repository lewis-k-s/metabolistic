# Plan: secretion abilities (area clouds)

Status: agreed 2026-10-18, implementation started. The biology-critic agent
checked the biology, and this version includes its corrections. All numbers
are first guesses. They all need a **playtest**.

Decisions of the review: a **toggle**, not a hold (press Q or E to switch a
secretion on, press again to switch it off). No rival strain yet. The gene
sources of question 3 are agreed.

## Implementation (2026-10-18)

Steps 1 to 8 are in the code (surfactin is not). Module `src/secretion/`;
tests in `src/secretion/*.rs`, `src/npc.rs`, `src/engulf.rs`, `src/envgrid.rs`
and `tests/it/secretion_test.rs`. Presets: `vent_chemist` (microcin, slime)
and `shallow_chemist` (siderophore, AI-2). Differences from the plan below:

- **Slime** has no mat crawl (`SLIME_MAT`). The drag multiplier alone (up to
  6 times the drag) slows a predator enough. Add the crawl if a playtest
  needs it.
- **The vent field has three symbionts** (the plan said none). AI-2 works in
  the vents too, but its gene comes from the shallow symbionts.
- **Gene sources**: two vent prey carry `cvaC`, two carry `pelA`; the two deep
  shallow prey carry `entF`; the shallow symbionts carry `luxS` (in place of
  `FatStorage`, which the vent symbionts still carry). Engulf one to get the
  gene; later runs can pick it on the Prepare screen.
- **Rust**: in the shallow and land fields, `RUST_SHARE_OXIC` (60 %) of the
  metal spawns are rust (`FieldKind::rust_share`). A rust pickup holds 2
  Metals. Its sprite is the metal sprite in a rust tint until it has art.
- **Drawing**: a separate quad over the field (`world/cloud_view.rs`,
  `assets/shaders/clouds.wgsl`), not the lighting shader. It is in the water
  layer (z -3.5): cells and pickups are drawn over it, and the darkness
  covers it like the rest of the world. Two cloud maps: A = microcin, slime,
  AI-2; B = free siderophore, iron complex. Each cloud is a smooth haze on
  the sprite pixel grid with a soft edge near the producer (the one blended
  effect of the world, at the user's request: the first version, sparse
  pixel specks, was too wide and too thin). Colour and pattern tell them
  apart: microcin a green sliding hatch, slime pale gel cells, free
  siderophore dark-gold swirls, the iron complex light-gold beads that
  glint, AI-2 mint rings that move out from the caller. No release puffs
  yet.
- **Tighter clouds** (second playtest): microcin diffusion 1.5, decay 0.3,
  damage 12 per unit; siderophore diffusion 1.0, decay 0.2; AI-2 diffusion
  2.0, decay 0.25, quorum 0.3. Each cloud now has a scale of about 70 to
  90 px.
- **Manage map**: the microcin and slime plates sit in the intake lane (under
  the membrane); siderophore and AI-2 take the two free plate places. Q and E
  cycle the keys there, and two buttons show them.
- **Icons**: `pathway_icon_{microcin,slime,siderophore,autoinducer}.png` from
  `art/gen_sprites.py`.
- **Playtest** (scripted, `src/playtest.rs`, 2026-10-18): all four clouds
  show and the keys work. Chelation went up to 0.6, and the cloud map
  scales (`CLOUD_FULL`) follow the levels that the playtest logged.
- **Known**: in quorum the symbiont exchange is 3 times faster, and it moves
  ATP from a rich player to the symbionts (it is a balance flow). With a full
  ATP pool this is a drain of about 25 ATP/s. Look at it when the symbiont
  trade is tuned.

## Problem

All the active membrane proteins that hit something use the aim: toxin, pilus,
injector and lyse. They compete for the cursor and for the four mouse buttons.
The cell has no ability that works round itself and needs no direction.

## Goals

1. Area abilities. The cell releases a substance into the water round it. The
   substance spreads, drifts and decays in the `EnvGrid`. Its effect comes
   from the level that a cell samples at its position.
2. Real biology for each one, from bacteria and archaea, not from animals.
3. No aim, no mouse button and no membrane slot. They do not compete with the
   aimed proteins.
4. The rules are pure functions. Headless tests check them with
   `MetabolisticApp::new_headless()`.

Out of scope: NPCs that secrete, siderophore theft by NPCs, microcins with
other kill modes. The design keeps a hook for each one.

## Why not chemokines

Chemokines are small proteins of vertebrates. They call immune cells to a
site. Bacteria have no chemokines, but they have the same two parts:

- **A signal that other cells sense**: quorum-sensing autoinducers. AI-2 (made
  by LuxS) is sensed by many species.
- **A response that moves cells**: chemotaxis up a gradient. *E. coli* swims
  towards AI-2 (receptor Tsr with LsrB), and this makes the cells form clumps
  (Laganenka et al. 2016, Nat Commun). The `Receptor` protein of the game is
  Tar, a receptor of the same family.

So the "chemokine" of this game is AI-2 (see "4. AI-2 call"): a chemical call
that brings helper cells to a place and changes what they do. The other
abilities are a bacteriocin (a weapon), a slime (a defence) and a siderophore
(a public good).

## The common mechanic

All secretion abilities use one mechanic. Each part reuses code that exists.

### A gene, not a membrane protein

Each ability is a **gene** (`BlockKind`). It is not a `MembraneProtein`, so it
uses no membrane slot. In real cells a secretion system is a gene cluster: the
genes that make the product, the export pump, and (for a weapon) the immunity
protein. The game puts the whole cluster in one gene.

The cell gets the gene like other genes: from the Codex (prepare points), or by
engulfing a cell that carries it (gene transfer).

### A block whose turnover goes out

The gene makes a **secretion block**. The block is like vesicle export: its
`FluxProfile` has demands only (ATP and one material) and no product inside
the cell. The solver runs it, so:

- the costs are real flux, and stoichiometry is conserved;
- a pool never goes below 0: with too little ATP the block slows down, and the
  cloud is thinner;
- enzymes make it faster, a mutated gene runs at half rate, and `Enhanced` is
  stronger. These rules exist now.

The block has `Drive` 0, so it does nothing by default. While its secretion is
on (the key toggles it), `secretion_drive_system` (in `MetabolicSet::Regulate`) sets `Drive` to
1. The pathway level of the Manage map (Off, Low, High) still multiplies it.

At the end of the tick, `secretion_exchange_system` (in `MetabolicSet::Exchange`)
reads the block's `BlockTurnover` and puts `turnover x YIELD` into the grid at
the cell's position (`EnvGrid::deposit`). This is the source step. The product
leaves the cell, so the ledger books it as an export.

### A grid channel for each product

Each product is a new `Channel` of the `EnvGrid`. The grid already does all
the work: diffusion, drift, decay, walls, leaky mats, `deposit`, `take` and
`sample`. A wall stops a cloud. A mat slows it. The drift of the water carries
it downstream, so the cloud trails behind a moving cell.

New channels need an arm in each `field_params` (vents, shallow, land) and in
the Manage screen matches. Their params are the same in each field, except the
drift, which copies the `Minerals` drift of that field.

An alternative is a second grid for secretions only, with its own enum. It
would touch fewer matches, but it would copy the walls, the leak mask and the
step code. One grid is one source of truth, so the plan uses `Channel`.

Cost of the grid step: the vent field is 64 x 48 = 3072 grid cells. Five more
layers add a few thousand multiply-adds to each sub-step. To keep it small,
`EnvGrid::step` skips a layer that is all zero and has no emission (a
`dormant` flag, cleared by `deposit`). Most of the time no cloud exists.

### Effects read the sampled level

Each effect is a pure function of the level at a position and the tick length
`dt`. If an effect uses up the substance, it calls `EnvGrid::take` and uses
exactly what it got, so mass is conserved. Each rule uses an exact decay
(`1 - exp(-k x dt)`), so the tick length does not change the result
(`time_scale_consistency`).

NPC steering reads the gradient of a channel with `npc::gradient`, as it does
for minerals, heat and light now.

### Keys: Q and E, toggle

The abilities need no cursor, so they go on the left hand, next to WASD: **Q**
and **E**. This is the `SecretionBar`, two slots. It is separate from the
`ActionBar`, so the four mouse buttons stay free for the aimed proteins.

- **Press** to switch a secretion on, press again to switch it off. The
  cloud stays after you switch off: it decays over a few seconds. While on,
  the block pays its costs each second, so watch the pools.
- The bar fills itself with the first two secretion genes. In Manage, the
  player cycles a key like an action slot.
- The HUD shows one line for each key, like the burst line:
  `[Q] Microcin: 1.5 ATP/s + 0.4 AA/s`.
- Direct input, like the burst on Shift (`world::actions_view::slot_hotkeys`).
  It flips a `SecretionOn([bool; 2])` component.

Q and E are free on the Run screen now (only the TUI uses E).

## The abilities

| # | Ability | Gene | Stage | Role | Cost per second on |
|---|---|---|---|---|---|
| 1 | Microcin cloud | `cvaC` | vents | weapon against prey and rival strains | 1.5 ATP + 0.4 AminoAcids |
| 2 | Slime (EPS) | `pelA` | vents | defence against the predator | 1.0 ATP + 0.6 CarbonSkeletons |
| 3 | Siderophore | `entF` | shallow | resource grab: Metals | 1.0 ATP + 0.6 CarbonSkeletons |
| 4 | AI-2 call | `luxS` | shallow | allies: symbionts gather and clump | 0.5 ATP + 0.2 CarbonSkeletons |
| - | Surfactin (later) | `srfAA` | land | swarming on moist ground | 1.0 ATP + 0.3 Lipids + 0.3 AminoAcids |

Each stage gets two: the vents a weapon and a defence, the shallow water a
resource grab and an ally call. The vents need a defence that works without
the AI-2 gene, which the player gets only in the shallow water.

### 1. Microcin cloud (`cvaC`)

**Biology.** Bacteriocins are protein toxins that bacteria release to kill
close relatives. Microcin V is a small peptide bacteriocin of *E. coli*
(gene `cvaC`). A type I pump (CvaA, CvaB and TolC) exports it, and the
producer stays alive. The target takes it up through its outer-membrane
receptor Cir (with TonB), and it breaks the target's inner membrane. The
producer makes an immunity protein (`cvi`) from the next gene. A microcin acts
only on close Gram-negative relatives, which have the receptor.

Why not colicin E1: *E. coli* releases colicin E1 by lysis. The lysis protein
kills the producing cell. That is suicide for a part of the population, not
something a single cell can keep on. A pumped microcin fits a secretion that
stays on.

**Channel.** `Channel::Microcin`. Diffusion 2.0, decay 0.3 (half-life about
2.3 s). The steady cloud round a still cell has a scale of `sqrt(D / k)` =
about 2.6 grid cells (about 80 px).

**Effect.**

- **Who it hits**: only the "relatives": prey, and the rival strain (see the
  questions). The predator (a protist grazer), the gate cell (a green sulfur
  bacterium) and the symbiont have no receptor, so they take no damage and
  take up nothing. A `Susceptible` marker on the NPC kinds says this.
- **Uptake**: a susceptible cell takes up microcin:
  `taken = take(Microcin, pos, level x (1 - exp(-MICROCIN_UPTAKE x dt)))`. The
  cloud is used up as it kills (each hit uses molecules).
- **Damage** = `taken x MICROCIN_DAMAGE_PER_UNIT` membrane points, as
  `DamageSource::Microcin`. Shelters do not stop it (it is a chemical, not
  heat).
- **Immunity**: a cell whose genome has `cvaC` (any state except Silent) takes
  no damage and takes up nothing. The player is immune to its own cloud. If
  the player engulfs a cell that carries `cvaC`, it gets the gene and the
  immunity together.
- **Avoidance**: prey steer down the microcin gradient when the level is above
  `MICROCIN_AVOID`, like `heat_escape`. Bacteria do swim away from many
  harmful chemicals.

**Role.** Hunting and competition, not defence. It finishes prey without the
aim (stun with the injector, then switch on Q). It clears prey out of a feeding
area, or pushes a group of prey away from a corner, so that the player can
take the minerals there. Against a rival strain that also makes it, only an
immune cell can stay in the cloud.

**Cost.** ATP and AminoAcids. Microcin V is a ribosomal peptide, and the
ABC pump uses ATP. It has no metal cofactor, so a Metals cost would be wrong.
AminoAcids make it compete with protein synthesis, which is a different trade
from the toxin and the injector (they use Metals).

**Hooks.** Bacteriocins with other kill modes: a DNase colicin (it needs a
metal ion, so Metals cost; effect: mutation load in the target), an RNase
colicin (slower protein synthesis in the target). `Enhanced` could pick one.
A vent-flavoured variant is sulfolobicin, an archaeal bacteriocin of
*Sulfolobus* that leaves the cell in membrane vesicles.

### 2. Slime (`pelA`)

**Biology.** Many bacteria secrete extracellular polysaccharides (EPS): the
slime of a biofilm. The Pel polysaccharide of *Pseudomonas aeruginosa* (gene
`pelA`) and bacterial cellulose (`bcsA`) are examples. The matrix makes the
water round the cells thick, holds the cells together and protects them from
grazers. Grazing resistance of biofilms and microcolonies is well documented
(Matz and Kjelleberg 2005, Trends Microbiol).

**Channel.** `Channel::Slime`. A polymer, so slow: diffusion 0.3, decay 0.05
(half-life about 14 s), drift 0.2 x the `Minerals` drift. It stays where the
cell made it.

**Effect.**

- A cell in slime has more drag. The multiplier is
  `1 + SLIME_DRAG x s / (SLIME_HALF + s)`, where `s` is the slime level at
  its position. It multiplies the drag of the field (`drag_in_field`), like
  the cuticle does.
- The producer is exempt (a cell with `pelA`, any state except Silent). This
  is a game rule: a real producer sits in its own matrix. **Playtest** whether
  the game needs it.
- (Not built.) Above `SLIME_MAT` the slime acts like a microbial mat for the other cells:
  they crawl at `MAT_CRAWL_SPEED` (see `walls`). The grid already has the mat
  rule; slime reuses it for bodies, not for diffusion.
- The slime is not used up. It only decays.

**Role.** Defence in the vents. When the predator comes, switch on Q and swim
away: the predator hits the slime trail and slows down. Or stay still in a
thick cloud: the predator crawls, and the injector or the pilus has time.
Slime also traps prey that swim into it, so it helps a hunter too.

**Cost.** ATP and CarbonSkeletons. A polysaccharide is made of sugars
(nucleotide sugars, made with ATP or UTP). It competes with growth (see
`docs/engulf-growth-plan.md`), which also uses spare carbon.

### 3. Siderophore (`entF`)

**Biology.** In water with oxygen, iron is Fe(III), which forms insoluble
oxides such as ferrihydrite. Bacteria release siderophores, small molecules
that bind Fe(III) very strongly and pull it off mineral surfaces.
Enterobactin is the siderophore of *E. coli*. EntC, EntB and EntA turn
chorismate (the end of the aromatic precursor pathway) into
2,3-dihydroxybenzoate. EntE and EntF join three of these with three serines
into a ring. EntS and TolC export it. The cell takes the iron-siderophore
complex back in through the outer-membrane receptor FepA. Inside, the
esterase Fes breaks the siderophore to free the iron, so each molecule is
used once. The siderophore is a public good: a neighbour with FepA can take
the complex too.

**Stage.** The shallow, oxic water. In the anoxic vents, iron is Fe(II) and
FeS minerals (mackinawite, pyrite). Enterobactin binds Fe(II) weakly and does
not dissolve sulfide minerals. Real vent cells take Fe(II) in directly
(through FeoB). So the vent metal pickups stay as they are: FeS that a cell
takes by contact. The shallow field gets a new pickup: **rust** (ferrihydrite).
A cell cannot take rust by contact. Only a siderophore dissolves it.

**Channels.** Two:

- `Channel::Siderophore` (the free chelator): diffusion 1.5, decay 0.15
  (half-life about 4.6 s).
- `Channel::FeSiderophore` (the loaded complex): diffusion 1.0, decay 0.02
  (stable: half-life about 35 s).

**Effect.**

1. **Chelation** (pure function `chelate(remaining, level, dt)`): a rust
   pickup in the cloud loses Metals at a rate that grows with the free
   siderophore level there:
   `dissolved = remaining x (1 - exp(-CHELATE_RATE x level x dt))`, and not
   more than the free siderophore that the grid gives (`take`, one
   siderophore for each Metal). The dissolved Metals go into `FeSiderophore`
   at the pickup (`deposit`). A pickup with nothing left despawns. Rust
   pickups have a `remaining` amount.
2. **Uptake**: a cell with `entF` (it has FepA) takes `FeSiderophore` at its
   position, up to `FE_UPTAKE_PER_SEC x dt`, into the `Metals` pool. One unit
   of complex gives one Metal. A cell without the gene takes nothing.

Every Metal that leaves a pickup comes into the grid, and every unit that
leaves the grid comes into a pool, so Metals are conserved.

**Role.** Resource grab at a distance. Stop near a cluster of rust pickups
and switch on E. The pickups dissolve into a gold haze that drifts downstream.
Swim through the haze to collect it. You need not touch each pickup, and you
can take pickups that sit in a hazard from a safe place downstream. The cost
is carbon now for Metals later: a bad trade if no rust is near.

**Cost.** ATP and CarbonSkeletons. Chorismate comes from central carbon, and
the joining steps use ATP. A small AminoAcids cost for the serine is
possible, but not needed.

**Hooks.**

- Piracy: an NPC with FepA but not the gene steals the complex. This is a real
  cheater strategy, and the grid makes it easy later.
- Photolysis: many marine siderophores (aerobactin, petrobactin,
  vibrioferrin) break in light, and the iron comes free. If the game uses one
  of them instead of enterobactin, the `FeSiderophore` decay can grow with the
  `Light` level. Enterobactin does not do this, so it is not in the first
  version.

### 4. AI-2 call (`luxS`)

**Biology.** Quorum sensing: each cell releases a small signal. When the
local level is high, there are many cells, and they switch genes on together.
LuxS makes AI-2 from the methyl cycle: SAM gives SAH, Pfs makes
S-ribosylhomocysteine, and LuxS makes DPD, which closes into AI-2. Many
species sense AI-2. (Some species have `luxS` only for the methyl cycle, so
"universal signal" is too strong.) *E. coli* swims towards AI-2 and forms
clumps (Laganenka et al. 2016). Clumps and microcolonies are too large for
many grazers.

**Channel.** `Channel::Autoinducer`. Small molecule: diffusion 3.0, decay
0.2 (half-life about 3.5 s). Scale about 3.9 grid cells (about 125 px).

**Effect.**

- **Recruit**: a symbiont steers up the AI-2 gradient when the level at its
  position is above `AI2_SENSE`. Symbionts follow the player now in any case,
  so this matters for symbionts that are far away or behind a wall gap.
- **Quorum**: when the level at a symbiont is above `AI2_QUORUM`, the
  symbiont is "in quorum". It closes in to contact (follow gap 0), and the
  symbiont exchange runs at `QUORUM_RATE_MULTIPLIER` x `SYMBIONT_RATE`.
- **Clump**: the player and the symbionts in quorum and in contact count as
  one clump for the engulf size rule. Each such symbiont adds
  `CLUMP_RADIUS_BONUS` to the player's radius in `can_engulf` (not to the
  body). With two or three symbionts, the predator cannot start an engulf.
- **Eavesdrop** (a game rule): a predator steers up the AI-2 gradient when
  the level is above `AI2_EAVESDROP`, also from beyond its sense radius. This
  is an analogy, not a fact about AI-2: the real case is *Myxococcus
  xanthus*, which senses the AHL signals of its prey (Lloyd and Whitworth
  2017). It makes the call a choice: it brings your allies and your enemy.

**Role.** Allies and defence in the shallow water, where the symbionts live.

**Cost.** ATP and CarbonSkeletons. The carbon is the ribose that came from
ATP when the cell made SAM.

**Hook.** Quorum quenching: the Lsr system takes AI-2 up and breaks it down.
A later NPC (or a gene of the player) can silence the call.

### Later: surfactin (`srfAA`, land)

**Biology.** Surfactin is a cyclic lipopeptide (a C13 to C15 hydroxy fatty
acid and seven amino acids). The operon `srfAA` to `srfAD` and the gene `sfp`
make it. It lowers the surface tension of the water film. With flagella,
*Bacillus subtilis* then swarms fast over a moist surface. Without flagella
it only "slides", slowly.

**Effect.** `Channel::Surfactant`: slow diffusion (0.5) and slow decay (0.1),
a film that stays where the cell went. On land, in moist areas (a water level
between `SURF_WET_MIN` and the puddle level), the drag multiplier of the
field (`land::drag_multiplier`) falls by `s / (SURF_HALF + s)` of the gap to
the water drag. The full bonus needs a `Flagellum`; without one the bonus is
`SLIDE_SHARE` of it. No effect on dry ground or in open water.

**Role.** Movement on land: fast paths along the moist ground between
puddles.

## Drawing the clouds

The clouds must be visible in the dark vents, but they must not look like
light. They use the same idea as the mineral specks in
`assets/shaders/lighting.wgsl`.

- **Cloud map**: a second RGBA8 texture, one texel for each grid cell, made by
  a pure function `render_cloudmap(grid)` in `lighting.rs` (beside
  `render_lightmap`). R = Microcin, G = Siderophore + FeSiderophore, B =
  Autoinducer, A = Slime. Each is scaled by its `*_FULL` level. A stage never
  has more than four clouds, so surfactin later takes a channel that the land
  stage does not use. `world::lighting_view` updates the map with the
  lightmap.
- **Shader**: the tint pass of `lighting.wgsl` reads the cloud map. Each
  cloud is a sparse speck pattern (hash of the sprite pixel, density from the
  level), drawn in one palette tone, never as a blended wash. The specks
  drift slowly with time, so a cloud looks like a haze of small molecules.
  They are dim in the dark and brighter in light, like the mineral specks.
  Slime is the exception: dense, still, larger dots, so it reads as a gel.
- **Colours** (rule 5, colour means role):
  - Microcin: the `toxin` ramp (acid green), the family of the toxin protein.
  - Slime: the `phb` ramp (pale storage polymer).
  - Siderophore: the free chelator in the `metal` shadow tone (faint gold),
    the loaded complex in the `metal` light tone. The haze turns gold where
    it picked up iron. Rust pickups use the `pigment` shadow tones (rust
    brown), with a `metal` glint.
  - AI-2: the `player` ramp (mint). It is the "self" signal, as the `self`
    antigen is mint.
  - Surfactant: the `head` ramp (lipid pink), thin.
- **Puffs at the cell** (`world::vfx`, `bevy_enoki`): while a secretion is on, a
  few small puffs in the cloud colour leave the rim of the cell and fade with
  dither. They show the moment of release. The grid cloud shows the rest.
- **Damage**: a cell that takes microcin damage flickers on its rim, like the
  UV burn.

## Files each change touches

| Change | Files |
|---|---|
| New channels, dormant skip | `src/environment.rs` (`Channel`), `src/envgrid.rs` (`step`, `deposit`), `src/level.rs`, `src/shallow.rs`, `src/land.rs` (`field_params`), `src/screens/manage/panel.rs`, `src/screens/manage/diagram.rs` (matches) |
| New genes and blocks | `src/blocks/genome.rs` (`BlockKind`, description), `src/blocks/mod.rs` (spec, marker, profile, `Drive` 0), new `src/blocks/secretion.rs` (profiles, `YIELD`, pure drive rule), `src/codex_catalog.rs` (`gene_facts`, `gene_symbol`), `src/genome_map.rs` (if a locus needs a look) |
| Secretion bar and keys | new `src/secretion.rs` (`Secretion`, `SecretionBar`, `SecretionHeld`, HUD line, plugin), `src/world/actions_view.rs` (Q and E input), `src/world/hud.rs` (lines), `src/screens/manage/mod.rs` (cycle a key), `src/scenario.rs` (bar in the file) |
| Source step | `src/secretion.rs` (`secretion_drive_system` in Regulate, `secretion_exchange_system` in Exchange), `src/metabolism/ledger.rs` (`FlowSource::Export` if the ledger needs it) |
| Microcin | `src/secretion.rs` (`microcin_uptake`, `microcin_damage`, immunity), `src/npc.rs` (`Susceptible` per archetype; `Senses` gets the microcin level and gradient; prey avoidance), `src/cell.rs` (`DamageSource::Microcin`) |
| Slime | `src/secretion.rs` (`slime_drag`), `src/world/player_body.rs` (sample the slime, multiply the drag, mat crawl above `SLIME_MAT`) |
| Siderophore | `src/secretion.rs` (`chelate`, `fe_uptake`), `src/particles.rs` (`RustPickup` with `remaining`, spawn zones, despawn when empty), `src/shallow.rs` (rust zones) |
| AI-2 | `src/npc.rs` (`Senses` gets the AI-2 level and gradient, symbiont and predator steering, quorum rate), `src/engulf.rs` (clump bonus in the size check) |
| Surfactin (later) | `src/land.rs` (`drag_multiplier` with surfactant), `src/world/player_body.rs` (sample it) |
| Drawing | `src/lighting.rs` (`render_cloudmap`), `src/world/lighting_view.rs` (second texture), `assets/shaders/lighting.wgsl` (cloud specks), `src/world/vfx.rs` (release puffs), `src/world/npc_view.rs` or a new view (rust sprite), `art/gen_sprites.py` (`pickup_rust`), `docs/art-direction.md` (colours) |
| Presets and docs | `assets/scenarios/vent_chemist.json` (microcin and slime), `assets/scenarios/shallow_chemist.json` (siderophore and AI-2), `CLAUDE.md` (keys, module list) |

## Order of work

Each step ends with `cargo th` green. A step that touches graphics ends with
`cargo test` and the smoke test.

1. **Channels and the dormant skip.** Add the channels with params in all
   three fields. Tests: a deposit spreads, decays and conserves mass with
   walls; a dormant layer is skipped and wakes on `deposit`.
2. **Secretion blocks and the source step.** Genes, blocks with `Drive` 0,
   `SecretionOn`, the drive and exchange systems. Tests: 1 s on
   puts `turnover x YIELD` into the grid and takes the costs from the pools;
   no ATP gives no cloud; 0.1 s and 0.8 s ticks give the same total.
3. **Microcin.** Uptake, damage, immunity, susceptibility, prey avoidance.
   Tests: a susceptible cell in a cloud loses membrane, and the cloud loses
   what it took; an immune cell and a predator take nothing; prey in a strong
   cloud steer away.
4. **Slime.** Drag multiplier, producer exemption, mat crawl. Tests: the
   multiplier grows with the level and saturates; the producer is not slowed.
5. **Keys, HUD, Manage, scenarios.** The `vent_chemist` preset. Smoke test.
6. **Drawing.** Cloud map, shader specks, release puffs.
7. **Siderophore and rust.** Chelation, uptake, rust pickups in the shallow
   field. Tests: Metals are conserved from pickup to grid to pool; a cell
   without `entF` gets nothing; no siderophore, no chelation.
8. **AI-2.** Recruit, quorum, clump, eavesdrop.
9. **Surfactin.** With the land stage tuning.

Steps 3 and 4 are independent and can run in parallel sessions after step 2.
So are steps 7 and 8 after step 5.

## Open questions

1. **The rival strain** (not now). The microcin hits only prey now. Add a "rival"
   strain NPC in the vents: a cell of the player's size that carries `cvaC`,
   uses it on the player, and gives the gene (and the immunity) when the
   player engulfs it?
2. **Where the other genes come from** (agreed): `pelA` from the Codex in
   the vents; `entF` from a shallow prey strain; `luxS` from the symbiont
   (its signature gene is `FatStorage` now).
3. **Slime exemption.** Keep the producer exempt from its own slime (a game
   rule), or slow it too (real, but the defence then needs more skill)?
4. **Scope of the first pass.** Do the vent pair (microcin and slime, steps
   1 to 6) first, and the shallow pair later?

## Ideas not taken

- **Colicin E1.** It is released by lysis, which kills the producer. See
  "1. Microcin cloud".
- **Siderophore in the vents.** Wrong chemistry for anoxic vents. See
  "3. Siderophore".
- **Amino-acid lure** (release aspartate, prey swim to it with Tar). Real and
  simple, but prey in the game flee a cell that can engulf them, so the lure
  only brings them to the edge of their sense radius. It also costs the
  amino acids that the cell needs most.
- **A cloud form of the existing toxin** (alpha-hemolysin). It would make the
  aimed toxin and the cloud the same ability.
- **AHL quorum sensing for the player alone.** A single cell cannot reach a
  quorum, so AHL has no use without allies. AI-2 with symbionts covers it.
- **Kept for later:** a secreted protease (`aprE`, subtilisin) that digests
  dead cells and debris in the cloud into AminoAcids; the cCF10 pheromone of
  *Enterococcus faecalis* (`ccfA`), which makes cells clump and then transfer
  a plasmid. It would fit the gene-transfer loop.
