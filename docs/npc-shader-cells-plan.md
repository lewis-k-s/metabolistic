# Plan: NPC cells as pixel-snapped shader bodies

Status: phases 0-2 built on branch `npc-shader-cells`, 2026-10-19 (see "Progress"). Phases 3-5 are open. The numbers are first guesses and
need a **playtest**. The species choices take creative licence; each row says
how close it is to the real organism.

## Problem

The player's body has a shader path (`world::shader_cells`, `cell.wgsl`): a
signed-distance-field (SDF) body on the low-res canvas, one texel per world
pixel. It grows, stretches and turns with no new art. The NPCs are still baked
sprite sheets (`prey_bacillus`, `predator_cell`, `symbiont_cell`,
`gate_cell_*`). This causes these problems:

1. **8 baked directions.** An NPC snaps between 8 rows when it turns.
2. **Fixed sizes.** A sheet has one radius. `engulf-growth-plan.md` makes cells
   grow, and sprites cannot follow without new sheets for each size.
3. **One look for each archetype.** A prey that carries Microcin, Slime,
   Cuticle or LightCapture looks the same as a plain prey. The player cannot
   see the key-gene prey (`layout.rs`, `land.rs`, `shallow.rs`) at a glance.
4. **No state on the body.** Membrane damage, stun and contact do not show,
   except a flat tint.
5. **Same clock.** NPCs of one archetype breathe in step.
6. **The cast is generic.** The art direction asks for real biology, and the
   research in `docs/research/deep-sea-vents.md` gives better models than
   "rod", "spiky ball" and "gold ball".

## Goals

1. Every NPC body is drawn by a pixel-snapped shader on the canvas, with the
   rules of `art-direction.md`: 5-tone ramps, light from the upper left, hard
   bands, a 1 px dark outline, dithered glow, no anti-aliasing.
2. The body shows the gene the NPC carries, its health and its intent.
3. The cast is a set of primordial cell types, close enough to real organisms
   that the Codex can name them.

Out of scope: the engulf pair clips (`engulf_player_prey`,
`engulf_player_symbiont`) and the gate victory clip stay baked for now. The
virus, particles and overlays stay sprites. NPC growth stays out (as in
`engulf-growth-plan.md`), but the shader takes any radius, so it is ready.

## Precondition: the canvas is the default

The canvas is a spike now: off by default, on with F7 or
`METABOLISTIC_SHADER_CELLS=1`. This plan needs it on for every Run screen.
Phase 0 does this. If another session promotes the canvas first, skip phase 0.

## Design

### One look, one shader

- **`src/cell_look.rs` (pure, headless-safe).** A `CellLook` is the body plan
  of a cell: shape, ramps, features and size factors. `look_for(archetype,
  signature, stage) -> CellLook` is a pure function. The player gets
  `CellLook::player()`. No wgpu types here, so tests run with `cargo th`.
- **`assets/shaders/cell.wgsl` becomes a general body shader.** The common parts
  (`lit_body` bands, outline, sparks, `BAYER4` dither) move to
  `cell_common.wgsl`. The shape and the features come from the uniform:

  | Shape | SDF | For |
  |---|---|---|
  | `Blob` | harmonics, optional spikes (today's `body_sdf`) | player, coccus, symbiont |
  | `Rod` | capsule | bacillus prey, proteobacterium gate |
  | `Curved` | arc capsule | vibrio prey |
  | `Chain(n)` | union of `n` rods or cocci | filament, tetrad, diplococcus |
  | `Colony(n)` | smooth union of `n` blobs in one halo | land gate |

  | Feature (bit flags) | What it draws |
  |---|---|
  | `NUCLEOID`, `GRANULES` | the dark discs inside, with the move lag |
  | `S_LAYER` | a 1 px dotted hex lattice on the rim: the mark of an archaeon |
  | `CAPSULE` | a dithered pale ring outside the outline (slime, EPS) |
  | `STRIPES` | thylakoid arcs (cyanobacteria) |
  | `CHLOROSOMES` | small ovals just under the membrane (green sulfur bacteria) |
  | `CRISTAE` | wavy inner folds (proteobacterium) |
  | `GLOW` | dithered halo that breathes |
  | `FLAGELLUM`, `ARCHAELLUM` | a sine line from the back pole, beats faster when moving |
  | `PROTRUSIONS` | 3-5 thin arms that reach towards the heading |
  | `PIGMENT_RIM` | the rim band takes a second ramp (carotenoid) |

- **One material per NPC.** About 20 NPCs on a level, a canvas of 428 x 240.
  The SDF is evaluated about 9 times per pixel (outline and normal), so the
  cost is small. Phase 1 measures it with the capture of `shader_cells`. If it
  is too high, move to one instanced material with a storage buffer.
- **Quad size** comes from the look: the body, the stretch, and the reach of
  appendages (flagellum, protrusions, capsule, glow). A test checks that the
  quad holds the largest pose.

### Motion and state (the QoL part)

| Uniform | Source | What the player sees |
|---|---|---|
| `angle` | velocity heading, smoothed | the body turns smoothly, no 8-row snap; light stays upper left (rule 9 holds) |
| `stretch` | `BodyDeform` (NPCs already have the pivot) | push stretch, as today |
| `phase` | time + a per-entity offset | NPCs do not breathe in step |
| `integrity` | `Membrane` / max | the rim thins and gets 1 px gaps below 50 %, so damage shows (rule 6) |
| `tint` | `stun_tint`, microcin hit | stun as a dithered cold tint, not a flat colour |
| `reach` | predator target distance | the arms extend towards prey before it strikes: a clear warning |
| `glow` | symbiont contact or AI-2 quorum | the halo brightens while the exchange runs |
| `life` | spawn / death clock | spawn grows from 0.4 to 1 with a dither fade-in; death is lysis: the outline breaks, the contents spill as dithered pixels, about 0.8 s |

Death moves to a detached quad with the same look (it replaces the baked
`death` clip). `NpcDied` needs the look key (archetype and signature).

### The cast

Bacteria have a smooth rim, archaea have the `S_LAYER` dots. This one cue
teaches the two domains. Tags follow the research doc: **[Faithful]** looks and
lives like the real organism, **[Licence]** keeps the idea and bends the rest.

| NPC | Model organism | Look | Tag |
|---|---|---|---|
| Prey, default (Fermentation) | *Clostridium*-like fermenter | periwinkle rod, polar flagellum (today's look, as a shader) | Faithful |
| Prey, Microcin | colicin-making rod | rod with two crimson toxin granules at the poles | Licence |
| Prey, Slime | capsulated rod | rod inside a dithered `CAPSULE` ring | Faithful |
| Prey, SecondaryMetabolites (vent pigment prey) | carotenoid rod | rod with `PIGMENT_RIM` that glimmers | Licence |
| Prey, Catalase (shallow) | aerotolerant diplococcus | two cocci in a `Chain(2)` | Licence |
| Prey, LightCapture (shallow, land) | filamentous cyanobacterium (*Anabaena*-like) | `Chain(3)` of short rods with `STRIPES`, slow glide, no flagellum | Faithful |
| Prey, Cuticle (land) | *Deinococcus*-like tetrad | `Chain(4)` cocci in a square, thick dark rim | Faithful |
| Vent chemolithotroph (new filler prey, optional) | *Campylobacterota* (vent sulfur oxidiser) | small `Curved` vibrio, fast flagellum | Faithful |
| Predator | Asgard archaeon (*Prometheoarchaeum*-like) | crimson `Blob` with `S_LAYER` and `PROTRUSIONS` that reach and cup the prey. Replaces the toothed mouth | Licence: Asgard cells have long arms, but hunting by engulf is speculation (eukaryogenesis story) |
| Symbiont | *Nanoarchaeum equitans* | tiny gold coccus, `S_LAYER`, `GLOW`; it rides on its partner in contact | Licence: the real one takes from its host; here it trades |
| Gate, vents | GSB1-like green sulfur bacterium | big teal `Blob`, orange `PIGMENT_RIM`, `CHLOROSOMES` under the rim instead of thylakoid stripes | Faithful (Beatty 2005) |
| Gate, shallow | alpha-proteobacterium | fat rose `Rod` with `CRISTAE` | Faithful |
| Gate, land | *Myxococcus*-like fruiting body | `Colony(4)` of small rods in one halo | Licence |

The Codex can name the model organism for each look later (one line in
`codex_catalog`); this plan does not add it.

### Files

- New: `src/cell_look.rs`, `assets/shaders/cell_common.wgsl`,
  `src/world/npc_body.rs` (attach, update, death quads).
- Changed: `shader_cells.rs` (canvas only; the body code moves to a shared
  body module), `cell.wgsl`, `npc_view.rs` (no `attach_body_for` sprite for
  NPCs; keep the collider part), `npc.rs` (`NpcDied` gets the signature),
  `art-direction.md` (rule 9, role table, sprite list, "In the game"),
  `art/gen_sprites.py` (stays the reference; NPC sheets leave the manifest in
  phase 5).

## Phases

| Phase | Work | Done when |
|---|---|---|
| 0 | Canvas on by default for the Run screen. F7 stays as a debug toggle. Cursor and aim use `canvas_cursor_world`. | Smoke test and `vent_predator` playtest pass with the canvas on. |
| 1 | `CellLook`, shared shader core, the 4 NPC looks of today ported 1:1 (parity). Player moves to `CellLook::player()`. Frame-time capture. | Screenshots match the sprites in shape and colour. Frame time is within 10 % of sprites. |
| 2 | Motion and state uniforms: smooth angle, phase offset, integrity, tint, spawn, lysis death, predator reach, symbiont glow. | Playtest screenshots show each state. Tests for the pure mapping (integrity -> rim, reach). |
| 3 | The prey variants from the cast table (shapes `Curved`, `Chain`, features `CAPSULE`, `PIGMENT_RIM`, granules). | Each key-gene prey is distinct in a gallery screenshot. |
| 4 | New predator and symbiont looks (`PROTRUSIONS`, `S_LAYER`), and the three gates. | Gallery and the three stage scenarios. |
| 5 | Remove NPC body sheets from the manifest (keep engulf and victory clips). Update `art-direction.md`. | `cargo test` passes; the manifest test passes. |

Phases 1-2 are one session. Phases 3 and 4 can run in parallel after phase 2
(they touch different looks, but both touch `cell.wgsl` features: give each a
separate feature block, or run them in order).

### Gallery

A dev launch, `METABOLISTIC_LOOK_GALLERY=1` with a scenario, spawns every look
in a row (frozen brains, invulnerable), moves each through idle, move, damage
and death, and saves screenshots to `target/gallery/`. It is the review tool
for the art owner and for the biology review. It exits by itself, like the
playtest.

## Tests

- `cell_look`: every archetype and signature in every level file gives a look;
  each key gene gives a look that is different from the default prey; the quad
  holds the largest pose with all appendages.
- `npc_body`: an NPC despawned in its first frame gets no quad (same as the
  sprite test today); a dead NPC leaves one lysis quad that goes after its clip.
- The smoke test, the playtest and the gallery for the pixels (read the PNGs).

## Decisions for you

1. **Predator.** Replace the toothed mouth with Asgard-style arms? (Recommended:
   yes. It is closer to biology and the reach is a better warning.)
2. **Vent gate.** Change thylakoid stripes to chlorosomes? Thylakoids belong to
   cyanobacteria, which do not live at vents. (Recommended: yes.)
3. **Archaea mark.** Use the `S_LAYER` dots for archaea only? (Recommended: yes.)
4. **Sprite fallback.** Keep the NPC sprite path while F7 can turn the canvas
   off, or remove it in phase 5? (Recommended: remove; F7 then turns off only
   the scaling, for debugging.)
5. **New filler prey.** Add the vent vibrio as a fourth prey look with no key
   gene? (Optional.)

## Progress

**Phases 0-2 (2026-10-19).** Shader cells are on by default
(`METABOLISTIC_SHADER_CELLS=0` turns them off). `src/cell_look.rs` holds the
body plans; the NPCs get theirs at spawn (`BodyLook`), the gate cells from the
level's gate sprite. The 4 NPC looks and both gate cells are ports of the
sprites. Phase 2 has the smooth facing, the loop offset, spawn growth, lysis,
the integrity rim and the symbiont contact glow. The predator "reach" waits
for the arms of phase 4. The look gallery is `METABOLISTIC_LOOK_GALLERY=1`.

Two fixes came with phase 0:

- `bevy_enoki` binds the view uniform buffer in `PrepareResources` with no
  order to `prepare_view_uniforms`. When the canvas opens, the buffer grows, and
  enoki could bind the old one (a wgpu validation error that quits the game).
  `shader_cells` runs `prepare_view_uniforms` once more in `PrepareViews`.
- The old spike hid the player's sprite through its `Visibility` and then
  copied that back to the body; only the engulf system kept the player
  visible. Now the sprites go to a render layer that no camera draws, and
  their `Visibility` is left to the systems that own it.

Not measured yet: the frame time with many NPCs on screen (the gallery ran at
about 50-60 fps offscreen, the same as without it). The decisions above are
still open.
