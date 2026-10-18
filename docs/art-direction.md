# Art direction

Status: first draft, 2026-10-15. It covers the vent stage only.

The game is 2D with pixel-art sprites (see `gameplay-roadmap.md`, "Movement"
and "Spatial environment"). The coarse environment grid and the pixel style
fit each other.

The view is **top-down**. Every sprite for the Run screen is seen from above,
with the light from the upper left. Side views are kept only for backdrops,
menu art and the codex (see `vent_chimney_side`).

## How to make the sprites

The sprites are drawn by code. This keeps the palette and the lighting the same
for every sprite.

```bash
uv run --with numpy art/gen_sprites.py
```

- Output: `assets/sprites/*.png`, native size, with transparency. Animation
  sheets are in `assets/sprites/anim/` (see "Animation" below).
- Previews: `docs/art/contact_sheet.png` (all sprites) and
  `docs/art/vent_scene.png` (a top-down mock of the Run screen).
- To change a color, change `RAMPS` in `art/gen_sprites.py` and run it again.
- Load the sprites with nearest-neighbour filtering
  (`ImagePlugin::default_nearest()`), or they will look soft.

## Rules

1. **One palette.** Each ramp has 5 tones: outline, shadow, base, light,
   highlight. A sprite uses one main ramp and a few small ones for the parts
   inside it.
2. **Light from the upper left.** Shapes are shaded as lit balls, with hard
   tone bands. No anti-aliasing and no partial transparency.
3. **1 px dark outline** on every sprite. The outline is the darkest tone of
   the sprite's own ramp.
4. **Colour means role.** The player must read a cell at a glance:

   | Role | Colour | Shape |
   |---|---|---|
   | Player | mint | round blob, nucleoid and bead inside |
   | Prey | periwinkle | small rod with a flagellum |
   | Predator | crimson | spiky body, toothed mouth |
   | Symbiont | gold | small, round, soft glow |
   | Gate cell (vents) | teal with an orange rim | big, thylakoid stripes |
   | Virus | lilac | hexagon head, stalk, legs |

5. **Resources have fixed colours** in every stage: carbon is warm tan,
   Minerals are cyan, Metals (Fe-S) are gold. Heat is orange and red. Cells
   never use these colours for their body, except the symbiont (gold) and
   the pigment rim of the gate cell (orange). Those two are small and have
   a clear shape.
6. **The membrane is a visible rim** on every cell: a lighter band at the
   edge. Later, its width and colour can show membrane integrity.
7. **Glow is dithered** (an ordered 4x4 pattern), not blended. It marks
   things that matter: the vent heat, the symbiont, the gate cell. Scene light
   and darkness follow the same rule (see "Light and darkness"). The one blend
   is a weak bloom, and only on emissive pixels.
8. **Walls are cross-sections.** A wall piece shows the bilayer cut open, as a
   band. This is the usual way to draw a wall from above, and it shows what the
   wall is made of.
9. **Body deform is baked into frames.** The cells animate with frames, not with
   a transform scale (see "Animation"). There is no soft-body physics: the
   deform is visual only. One exception: while a cell pushes in the direction
   it moves, the game stretches its body sprite along the motion, up to
   `MAX_MOVE_STRETCH` at full speed (area kept, see `world::player_body`). A
   cell that a blow or a current carries does not stretch. The frame never
   rotates.

## Sprite list

| File | Size | Roadmap use |
|---|---|---|
| `player_cell` | 48x48 | M2, the player `Cell` |
| `prey_bacillus` | 28x22 | M5, prey |
| `predator_cell` | 48x48 | M5, predator |
| `symbiont_cell` | 32x32 | M5, symbiont |
| `gate_cell_phototroph` | 72x72 | M5, gate cell of the vent stage |
| `gate_cell_proteobacterium` | 72x72 | M7, gate cell of the shallow-water stage (rose, cristae folds) |
| `virus_phage` | 44x28 | M5, virus (lies flat, points right) |
| `food_carbon` | 12x12 | M2, OrganicCarbon particle |
| `pickup_mineral` | 12x18 | M2, Minerals particle |
| `pickup_metal_fes` | 16x16 | M1/M2, Metals cofactor |
| `wall_membrane` | 16x26 | M5, world membrane (tiles sideways) |
| `wall_membrane_pore` | 32x26 | M5, wall with a transporter |
| `vent_chimney_top` | 128x128 | M2, vent core: Heat and Minerals source |
| `vent_plume_top` | 128x128 | M2, smoke overlay. Draw above creatures |
| `seafloor` | 128x128, tiles | Vent level: the deep-sea floor (sediment, pebbles, fine cracks, low contrast). Tiled under the grid image |
| `vent_chimney_side` | 72x120 | Menu, loading and codex art. Not for the Run screen |

## Animation

Idle and move are loops of 8 frames. All waves use whole turns of the
phase, so the loop is seamless (the generator checks this by drawing frame 8 and
comparing it with frame 0).

| Sprite | Idle | Move |
|---|---|---|
| `player_cell` | membrane waves, drifting organelles, slow breath | yes |
| `prey_bacillus` | flagellum wave, the rod sways | yes (rod turns to the heading, flagellum beats faster) |
| `predator_cell` | spikes pulse, mouth chatters | yes (mouth opens and points at the heading) |
| `symbiont_cell` | glow pulses, body breathes | yes |
| `gate_cell_phototroph` | stripes flow, halo pulses | no (it does not move) |
| `virus_phage` | legs twitch | no (it drifts) |

**Move deform.** The cell stretches along its heading, the back end tapers, and
the organelles lag behind. The stretch peaks about a quarter of the way into
the loop, then relaxes. This looks like the push of a flagellum or a cilium.
Light stays at the upper left, so each of the 8 directions is shaded again.
Rotating a frame at run time would turn the light too, and rotation breaks the
pixel grid. This is why the directions are baked.

### One-shot animations

These play once and then return to idle (or end, for death). They are not
loops. All of them use the same 8-frame rows, except death (10 frames).

| Kind | Frames | What it shows |
|---|---|---|
| `contact` | 8 rows (direction) x 8 | A squash against a flat wall. The front is cut flat on the wall plane, the body bulges to the sides, the organelles keep moving into the wall, then the cell rebounds and settles. |
| `hit` | 8 rows (direction) x 8 | A white flash (silhouette, then checker), impact rays, a squash on the side that faces the attacker, then a knock-back stretch along the line of the blow. |
| `spawn` | 1 row x 8 | The cell grows from a speck, overshoots to 112 %, and settles. A dithered ring spreads out, and the first frames flash. |
| `death` | 1 row x 10 | The body swells and flashes, cracks, and breaks into 9 shards that fly out, turn to grey-blue stone, and fade away in dither. Bright specks of cytoplasm fly out too. The last frame is empty. |

- **Contact direction** = the direction from the cell to the wall. Row `E` is a
  wall on the right. The wall plane is `radius` px from the anchor (see
  `manifest.json`). The sprite does not draw the wall. The squash is already in
  the frame, so the game should not scale the sprite on contact.
- **Hit direction** = the direction the blow travels (the knock-back direction).
  A blow that comes from the left and goes right is row `E`.
- The prey rod and the predator face the contact or the attacker, as they do
  when they move. The predator opens its mouth wide on a hit.
- `death` is made from frame 0 of the idle loop, so it fits every cell. The
  gate cell has no death: engulfing it is the victory.
- Hit and spawn flash with a white silhouette and keep the outline, so the
  shape always reads. There are no new colours: the flash is the only one extra
  tone (`FLASH` in the generator).

**Files** (`assets/sprites/anim/`):

- `<name>_idle.png`, `_spawn.png`, `_death.png`: one row of frames.
- `<name>_move.png`, `_contact.png`, `_hit.png`: 8 rows (one for each direction)
  of 8 frames. Direction order: E, SE, S, SW, W, NW, N, NE. These are screen
  angles in steps of 45 degrees, with +y down.
- `manifest.json`: frame size, columns, rows, fps, `loop`, directions (with their
  meaning), `anchor_px` (the cell centre in a frame) and `radius` for each sheet.
  A Bevy `TextureAtlasLayout::from_grid(frame_size, columns, rows, ..)` fits the
  sheets directly. The row for a heading is the nearest 45-degree step.
- Frame sizes are larger than the static sprite, to give room for the deform:
  64x64 for the player and predator, 48x48 for the symbiont and the prey,
  52x36 for the virus. The gate cell stays 72x72.

**Suggested use.** Play `move` when the speed is above a small threshold, and
`idle` below it. Pick the row from the velocity angle. Keep the frame index when
you switch between rows, so the push does not jump. Run `idle` at 6 fps and
`move` at 10 fps (more speed can raise the fps). Play the one-shots at the fps in
the manifest. Anchor every frame by `anchor_px`, not by the frame centre: the
prey frame, for one, is not centred on the body.

Previews: `docs/art/anim/idle.png`, `docs/art/anim/move_<name>.png` and
`docs/art/anim/preview.gif` (idle and move), `docs/art/anim/oneshots.gif` (contact
with a wall, hit, spawn, death), and `contact_<name>.png`, `hit_<name>.png`,
`spawn.png` and `death.png` in the same folder.

## Membrane protein overlays

The roadmap gives the cell actions through membrane proteins. Each protein is a
separate sprite that sticks out of the membrane, so the game can mount any of
them on any cell, at any slot. They are drawn above the cell.

| Overlay | Colour | Looks like | Idle | Use |
|---|---|---|---|---|
| `overlay_flagellum` | protein blue | a motor ring with a long wavy filament | small slow wave, motor turns slowly | burst of thrust: big fast wave, motor spins, specks stream off the tip. Loops while it lasts |
| `overlay_adhesin` | magenta | three stalks with knobs | the stalks sway | **use** (the grab, plays once): stalks reach and fan out, knobs flatten into glowing pads. **hold** (stuck, loops): taut stalks, pulsing pads, a bright pulse runs along each stalk |
| `overlay_toxin` | acid green | a sac with an injector needle | the sac breathes, a drop swells on the tip | fire (plays once): the sac squeezes, the needle thrusts, a spray leaves the tip |
| `toxin_droplet` | acid green | the projectile | 8-frame pulse | spawn it at the needle tip when the toxin fires |

- **Frame**: 56x56 for every overlay. The mount point is at the centre
  (`anchor_px` is 28,28). Put it on the membrane, 0.95 x the cell radius from
  the cell centre, in the direction of the slot.
- **Directions**: 8 rows (E, SE, S, SW, W, NW, N, NE). The row is the direction
  in which the protein points, away from the cell. Each row is shaded again, so
  the light stays at the upper left.
- **Sheets**: `<overlay>_idle.png`, `_use.png`, and `_hold.png` for the adhesin.
  Every sheet has 8 columns. `manifest.json` has the fps and `loop` for each.
- **Layer**: all overlays are `above` the cell. The flagellum motor ring then
  stays visible on the rim.
- **Why a separate frame, not baked into the cell**: the cell has many frames
  (idle, move, hit...) and a cell can carry any mix of proteins. Baking them
  together would need a sheet for every mix. The overlay only needs to start on
  the membrane. The small error from the cell deform is hidden by the socket
  ring at the base.
- **Colours**: the three overlay colours are for membrane proteins only. The
  flagellum uses the same blue as the transporter in the pore wall, because both
  are membrane machines.

### Exchange and sensing proteins

The second set. Same rules as above (56x56 frame, anchor at the centre, 8
directions, drawn above the cell).

| Overlay | Colour | Looks like | Idle | Use |
|---|---|---|---|---|
| `overlay_transporter_carbon`, `_mineral`, `_metal` | protein blue barrel, molecules in tan, cyan or gold | a short barrel with a channel and two gates. The same blue and the same rungs as the pore in the wall | gates shut, one molecule waits at the door | gates open, molecules stream in through the channel. Loops while it takes up |
| `overlay_lytic` | steel | a socket with three blades | blades nearly shut, breathing | blades open wide and saw, sparks fly from the tips. Loops while it cuts |
| `overlay_fusion` | ivory | a stalk with a head and a harpoon tip | the stalk sways | **use** (once): the harpoon shoots out, sticks, and the stalk folds back into a hairpin. **hold** (loops): the folded hairpin, with a pulse along it, while membranes merge |
| `overlay_receptor` | sky blue | a stalk that forks into two cups | sways, cups empty | (once) a molecule drifts in and drops into a cup, the cup lights up, a signal runs down the stalk into the cell |

- **A transporter for each substrate.** The molecule colour is the resource
  colour (rule 5): carbon, Minerals, Metals. It matches a wall pore for the same
  substrate. For a new substrate, add a ramp name to `SUBSTRATES`.
- **Shape tells them apart.** The colours of the overlays are close (blue,
  steel, ivory, sky), so the silhouette carries the meaning: a barrel takes in,
  blades cut, a long harpoon joins, a fork senses.
- The lytic enzyme costs a cofactor for each use (see the roadmap). Show the
  cost in the HUD, not on the sprite.

Previews in `docs/art/anim/`: `overlays.png` (every overlay and mode, pointing
east), `overlay_directions.png` (all 8 directions), `overlay_demo.png` and
`overlay_demo.gif` (the three overlays on the player: idle, use, then hold).
The second set has the same files with a `_2` suffix: `overlays_2.png`,
`overlay_directions_2.png`, `overlay_demo_2.png` and `overlay_demo_2.gif`.

### Immune proteins

The third set. Same rules as the first two (56x56 frame, anchor at the centre,
8 directions, drawn above the cell). Every sheet has `idle`, `use` and `hold`.

The body of every immune protein is **pearl**, a neutral grey-white. The colour
is in the **antigen**, the piece of something that the protein shows, looks
for or tags. An antigen has the colour of where it comes from (rule 5): `self`
is player mint, `viral` is virus lilac, `prey` is prey periwinkle. For a new
origin, add a line to `ANTIGENS`.

| Overlay | Looks like | Idle | Use (plays once) | Hold (loops) |
|---|---|---|---|---|
| `overlay_mhc1_self`, `_viral`, `_prey` | MHC class I: a stalk, a small ball (beta-2 microglobulin), a flat head with two ridges and a groove | the head sways, the groove is empty | a peptide climbs the stalk, slides into the groove, the ridges flash | the peptide is on show. A bright bead runs along it, the head glows in the antigen colour |
| `overlay_mhc2_self`, `_viral`, `_prey` | MHC class II: two stalks, a wider head, an open groove. The long peptide sticks out of both ends | a grey placeholder sits in the groove | the placeholder is thrown out and breaks up. The real peptide slides in from the outside | the peptide is on show, same glow |
| `overlay_tcr_self`, `_viral`, `_prey` | T cell receptor: a stalk with a cup at the tip, two signal knobs at the base. The inner rim of the cup has the colour of the antigen it matches | the cup sways and waits | a small MHC head with that antigen drifts in, the cup clamps on it with a flash, the knobs light up, a signal runs down the stalk into the cell | still clamped, the knobs light in turn, a pulse runs down the stalk |
| `overlay_antibody_self`, `_viral`, `_prey` | antibody: a Y with a stem in the membrane. The arm tips have the antigen colour | the arms flex | the arms open, an antigen ball drifts in, the arms close on it, it glows (tagged) | the tagged antigen stays in the grip and pulses |
| `overlay_perforin` | a stalk and a ring of eight parts | three parts of the ring (a C) rock, dots mark the other places | the other five parts fly in and click into the ring. A flash, then a dark pore with a red rim opens | the closed ring. A bright part runs round it, a red glow breathes |

- **What each one means in play** (for the design, the art does not decide
  it): MHC I shows what the cell makes inside, so a cell with a viral peptide
  gives itself away. MHC II shows what the cell ate, which fits the engulf and
  digest steps. The TCR reads a shown antigen on another cell. The antibody
  tags a free antigen. Perforin punches a pore in a membrane.
- **Red is only on perforin.** The red pore rim and glow say "this kills", as
  the predator colour does. It is the only place an immune protein uses it.
- **Idle has no antigen.** MHC shows an empty groove, the TCR and antibody
  show only their matching colour. `use` and `hold` are the states with
  cargo. The sheets for the three antigens of one protein have the same idle
  frames, so the game can swap by name.
- Colour on the cell: pearl reads on all the cell colours, and the antigen
  colour is small, so check lilac on the virus cell and periwinkle on the prey
  cell in game (not done yet, see below).

Previews in `docs/art/anim/`: `overlays_3.png` (one antigen, every kind),
`overlay_antigens_3.png` (the three antigens, `use` frame 5 then `hold`
frame 5, one row for each protein), `overlay_directions_3.png`,
`overlay_demo_3.png` and `.gif` (a cell that shows MHC and an antibody), and
`overlay_demo_4.png` and `.gif` (a cell with two TCRs, an antibody and
perforin).

## Engulf, digest and gene gain

A cell eats a smaller cell in three steps. Each step has its own sheet, and all
three have 8 rows. The row is the direction from the engulfer to the target.

| Step | Frames | Plays | What it shows |
|---|---|---|---|
| `engulf` | 12 (112x112) | once | The engulfer stretches to the target and two arms curl round it. The target struggles. The arms close into a ring. The ring becomes a vacuole, and it sinks into the body. The body gulps and settles. |
| `digest` | 8 (64x64) | loops | The vacuole sits inside the engulfer with the target in it. Specks drift in the vacuole. |
| `digest_end` | 10 (64x64) | once | The target flashes white, then dissolves. The vacuole shrinks away. A bright speck of gene (with a trail in the target's colours) flies to the nucleoid. The nucleoid flashes and a ring spreads. |

Pairs: `engulf_player_prey` and `engulf_player_symbiont`. The two cells touch in
the frame, so each pair has its own sheets. To add a pair (for example a
predator that eats prey), add a line to `PAIRS` in `art/gen_sprites.py`.

Game flow: play `engulf`, then loop `digest` for as long as the game digests,
then play `digest_end` (this is the moment of the gene gain), then return to
`idle`. The last frame of `engulf` and the first frame of `digest` show the same
vacuole, so there is no jump. The engulf frame is larger than the cell frame:
anchor both by `anchor_px` in `manifest.json` (the engulfer centre). Do not draw
the target cell yourself during these steps: it is in the frames.

The engulfer must be larger than the target. The size check is for the game.

### Gate cell victory

The gate cell is bigger than the player, so it is not wrapped. It gives up its
gene instead. `gate_cell_victory.png` (16 frames of 96x96, centred on the gate
cell, plays once):

- 0-1: the body shakes and the halo flares.
- 2-5: it swells and light leaks out of cracks.
- 6-9: it breaks into shards, and specks of light stream out.
- 8-15: the shards dither away. A gold orb (the gene) forms in the middle, and
  two rings spread out from it.

Then loop `gene_orb.png` (24x24, 8 frames, the same orb with sparks that circle
it) and fly it to the player. Gold-white is the colour of the reward, so it is
used for nothing else.

Previews in `docs/art/anim/`: `engulf_story.gif` (engulf, digest, gene gain, for
both pairs in two directions), `engulf_<pair>.png` and `engulf_<pair>_digest.png`
(sheets), `gate_victory.gif` and `gate_victory.png`, and `gene_orb.png`.

## Interior pieces

The cell sprite shows what the cell can do. Each expressed pathway adds one
small sprite piece **inside** the membrane, under the membrane protein overlays
(which stay on the rim). A piece is a separate sprite, so the game can pick,
place and swap pieces without a new body sheet. Every piece is a child of the
`BodyDeform` pivot, so it stretches with the body (art rule 9).

| Pathway (gene) | Piece | Real structure | Look |
|---|---|---|---|
| `Chemosynthesis` | `interior_sulfur` | sulfur granules, as in *Beggiatoa* | three bright yellow beads, a sparkle runs over them |
| `LightCapture` | `interior_thylakoid` | thylakoid or chlorosome stack | four green lamellae, a light band runs along them |
| `FatStorage` | `interior_lipid` | PHB-like lipid droplets | pale beads of two sizes, a highlight drifts |
| `ProteinSynthesis` | `interior_ribosome` | ribosome speckle, dense in the cytoplasm | a cloud of small dark-lilac dots that twinkle |
| `Catalase` | `interior_peroxisome` | peroxisome (catalase crystal) | two round bodies with a dark core, bubbles of O2 rise |
| `Respiration` | `interior_cristae` | folded inner membrane (cristae) | a rose oval with folds that sway |
| `SecondaryMetabolites` | `interior_pigment` | carotenoid ring under the membrane | orange dotted ring, **ring piece** |
| `Cuticle` | `interior_cuticle` | thicker outer wall layer | a thick tan rim on the outside, **ring piece** |

- **Gene state.** `Expressed` and `Enhanced` draw the `idle` clip. `Mutated`
  draws the `glitch` clip: the same shape, dim, with an offset row of pixels that
  jumps. `Silent` draws nothing. A gene with no enzyme draws nothing.
- **Frame.** A spot piece has a 16x16 frame (the shape fills about 10 px) with the
  anchor at the centre (8, 8). A ring piece has a 64x64 frame, the same as the
  player body, with the anchor at the body centre. Both have 8 frames and play as
  loops (`idle` at 5 fps, `glitch` at 8 fps). The light is from the upper left.
- **Slots.** A spot piece sits in one of 8 fixed slots on a ring of radius 12 px
  round the body centre, 45 degrees apart. The shapes are at most 10 px wide, so
  the neighbours do not touch. A ring piece has no slot (it is centred) and a
  cell shows each ring piece at most once. The fill order of the slots is
  NW, SE, NE, SW, E, W, N, S, so that the first three pieces spread round the
  body.
- **Selection** (`interior::interior_plan`, a pure function). Each candidate gets
  a score from its gene state (Expressed 1.0, Mutated 0.5), its enzyme count
  (up to 4) and its activity (the throttle of the block, 0 to 1). The best N are
  kept, N set by the display mode. A piece that is already shown gets a bonus
  (hysteresis), and it keeps its slot. A new piece takes the free slot with the
  lowest number. So a piece does not jump or flicker when two scores are close.
- **Display modes** (`CellDisplay`, key `V` on the Run screen, the HUD shows the
  mode for a few seconds):

  | Mode | Interior pieces | Membrane overlays |
  |---|---|---|
  | `Minimal` | none | up to 2 of a kind, 6 in all |
  | `Standard` | the best 3 | up to 2 of a kind, 6 in all |
  | `Full` (debug) | all candidates (at most 8 spots plus 2 rings) | up to 3 of a kind, 8 in all |

- **New piece.** Add one line to `PIECES` in `src/interior.rs` and one entry to
  `INTERIORS` in `art/gen_sprites.py`. A new membrane protein is not an interior
  piece: it goes in `actions_view::overlay_plan`.
- **NPC cells** use the same pieces when the body is big enough (radius of 18 px
  or more, not the gate cell, which has its own art). They are limited to
  `Standard`, with at most 2 spots and no rings, to keep the frame time low.

Previews in `docs/art/anim/`: `interior_pieces.png` (every piece, `idle` then
`glitch`, 8 frames) and `interior_demo.png` (the player with 3 and with 8 pieces).

## In the game

`src/sprites.rs` loads every sheet of `manifest.json` (the manifest is compiled
in, and a test checks that every PNG has the size the manifest says), and plays
them with an `Animator` component. `ImagePlugin::default_nearest()` keeps the
pixels sharp, and sprites are drawn at `PIXEL_SCALE` (3 screen pixels for each
sprite pixel). The Loading screen waits for the sheets.

On the Run screen the player is the `player_cell` sprite: `spawn` once, then
`idle`, or `move` with the row of the last heading (8 directions from WASD).
The frame carries over when idle turns to move. A flagellum overlay is mounted
at 0.95 x the radius, points away from the heading, and plays `use` while the
cell moves. It is a placeholder until the proteome says which overlays to draw.
`anchor_px` is rounded down to a whole pixel, so a sprite never sits on a half
pixel (the cell frames are centred on 32.5).

## Light and darkness

Status: 2026-10-18. Phase 1 (lightmap, darkness overlay, debug key F6) is in
the game, and so are the floor sprite and the vent particles (moved forward
from phase 3). The other phases are not.

**Changes after the first phase-1 build.** The grid image no longer draws the
Heat and Light bands (the orange glow round the vents and the yellow and
violet light zones). The lighting shows the vents and the UV shafts on its
own. The grid still draws O2, water and, on land, minerals. In the vents and in
shallow water the minerals are fine cyan specks (1 sprite pixel, a BAYER4
dither of the mineral level, at most 40 % of the pixels), drawn by the
lighting shader on the additive quad from the alpha channel of the lightmap.
The grid texel is 4 world pixels, which was too coarse for a dither. The vent level has a
textured floor sprite (`seafloor`, tiled at z -20, under the grid image), and
the darkness darkens it like the rest. The burn zone (heat from about 30 to
60) turns white in the light, so it is the brightest and whitest area. The
quads reach 1200 units past the field and the lightmap is clamped at its
edge, so there is no seam at the field edge.

The sprites carry their own light (rule 2), and the background is one flat
colour. This plan adds scene light: the field is dark far from a vent or a
light shaft, and some entities glow. The vent stage is the darkest. The
darkness is part of the play there.

### The light field comes from the grid

The game does not use light entities. The light at a point is a pure function
of the grid:

```
light(x) = ambient(level) + k_heat * Heat(x) + k_light * Light(x) + splats(x)
```

- `Heat` gives warm light (the `vent` ramp), `Light` gives cool light. In the
  vents all light is UV, so the shaft light is a pale violet (a blacklight).
- `splats` are small, soft additions from emitters: fluorescent cells, toxin
  droplets, stun flashes.
- The light falls off from a vent or a shaft with the same diffusion and drift
  as the simulation. The plume drift moves the glow, and the burn zone is the
  brightest area. There is no second falloff to tune.
- `ambient` is set for each level: vents about 0.08, shallow water a gradient
  with depth, land 1.0. Shallow water and land are bright, so the darkness is
  mostly a vent-stage look.
- The lightmap is a pure function of `EnvGrid`, the level and the emitter list.
  Test it without a window, like `render_grid`: light falls as the distance
  from a vent increases, it never goes below the ambient, and a shaft is lit.
- The light is for the player's view only. It does not change the simulation.

### How it is drawn

One camera. No lighting crate: the main light source is the grid, and each
crate is one more dependency that can hold back a Bevy upgrade.

1. **Lightmap.** After each metabolic tick, the CPU writes one RGB texel for
   each grid cell (or 2 x 2) into a small texture with linear filtering.
2. **Darkness overlay.** A `Material2d` quad above the world, with normal alpha
   blending: colour = the darkest tone of the `WATER` ramp, alpha = 1 - light.
   Cells fade into deep-sea navy, not into black. A second, additive quad
   adds the warm tint near the vents.
3. **Bands and dither.** The shader snaps each fragment to a sprite pixel
   (`PIXEL_SCALE`), sorts the light into about 5 bands, and dithers the band
   edges with `BAYER4`. A sprite pixel is either in a band or not. There is no
   soft gradient on the screen.
4. **Emissive layer.** Glow sprites are drawn above the darkness, so the
   darkness does not hide them. The camera has HDR and a weak `Bloom` with a
   threshold above 1. Only emissive pixels pass the threshold, so only they
   bloom.
5. **Debug.** A dev key turns the lighting off. The smoke test saves one
   screenshot with the lighting on.

Later (v2): a render target for small moving lights, and wall shadows from
`WallMap`.

**Baked light and scene light.** The sprites keep their upper-left light.
The scene light only changes how much of each sprite you see, and its tint.
The rim mask (see below) can later give a lit edge on the side of the cell
that faces a vent.

### Fluorescence

- **The biology.** Real vents give off a faint glow (thermal radiation and
  chemiluminescence). A green sulfur bacterium from an East Pacific Rise vent
  does photosynthesis with this geothermal light, which is a good origin
  story for `gate_cell_phototroph`. Fluorescence is light that a molecule
  gives back after UV excites it. The vent shafts are all UV, so an entity
  glows strongly inside a shaft.
- **Glow floor.** Outside the shafts, a fluorescent entity keeps a small,
  stylized glow. This is the one place where the look is more important than
  accuracy: without it, the glowing entities would be dark in the dark.
  Emission = floor + gain x local UV.
- **No bioluminescence in the vents.** Most luciferases need O2, and the vents
  have none. Bioluminescence can come later, in shallow water, as a lure or a
  signal.
- **Selective.** Glow gives information to the player. Each glow uses the
  highlight tone of the role's own ramp, so rule 4 stays true.

| Entity | Glow | Why |
|---|---|---|
| Prey | dim, periwinkle | food that you can find (flavins fluoresce) |
| Pigment prey | strong in UV, at the edge of shaft A | it shows the way to the pigment gene |
| Gate phototroph | red halo (bacteriochlorophyll) | the goal is a beacon |
| Symbiont | gold (its glow now) | - |
| Mineral and metal pickups | a glint when lit; minerals fluoresce in UV | real minerals fluoresce |
| Predator | none | tension: you see it as a shape against lit areas, and by its wake in the marine snow |
| Virus | none | a hidden threat |
| Player | none at first | a later light-sensing gene can make the view larger |

**Masks from the generator.** `art/gen_sprites.py` also writes, frame for
frame, `<name>_emit.png` (the pixels that fluoresce) and `<name>_rim.png` (the
membrane band). The atlas layout is the same as the main sheet, so a child
sprite uses the same atlas index. One material, `DitherGlow` (colour,
intensity, time), draws a mask as hard on/off pixels: a pixel is on when the
intensity is above its `BAYER4` threshold. The same material draws
fluorescence, the pigment glimmer and the UV burn sparkle.

### Pigments

- **Glimmer.** A cell with pigment shows a dithered sparkle on its rim mask, in
  the pigment colour. Intensity = local UV x shield (`P / (10 + P)`). The
  threshold offset changes with time, so the pixels twinkle. The sparkle shows
  that the shield works, without the HUD. As the pigment decays, the sparkle
  fades.
- **Burn.** A cell without pigment gets a damage flicker on the same rim
  instead. The contrast between the two teaches the rule.
- **Stages.** Vents: a rare glimmer in the dark. Shallow water: pigment is the
  main protection, and it also tints the body by pool size. Land: a full body
  tint, and the glimmer only in very strong UV.
- **Colour (open).** Orange (carotenoid) conflicts with Heat (rule 5). Violet
  conflicts with the UV light and the virus. Candidates: a rose-amber
  carotenoid, or a brown-gold scytonemin. Later the pigment type can be a
  choice of the player.

### Particles

Status: the vent smoke and sparkles are in the game (`world::vfx`). The rest
is not.

Use `bevy_enoki` 0.7 (it supports Bevy 0.19). It is made for 2D, works on
WebGL2 and hot-reloads its effect files. `bevy_hanabi` needs compute shaders,
so it does not run on WebGL2. The module is `world::vfx`: `particles.rs` is
already the pickups module.

- **Vent smoke and sparkles (in the game).** Each vent has a dense smoke
  column (dark grey-brown puffs, the `waste` ramp) in two layers: big puffs
  that grow from 10 to 48 px as they rise (9 s life), and thin wisps. The rise
  slows with age and the Heat drift of the grid bends the column, so it fans
  out at the top. It is in the lit layer (z 6, below the darkness). It also has
  sparkles (small light-orange and yellow specks, the `vent` ramp) that rise
  and twinkle out. Sparkles are emissive: they are drawn above the darkness
  (z 30). A replacement vertex shader snaps every particle to the sprite pixel
  grid, and the particle shader draws palette tones with a dither fade.
- A small particle material snaps each particle to a sprite pixel and uses
  palette colours only. Particles fade out with dither, not with alpha.
- Particles are drawn in the lit layer, so the darkness hides them, unless
  they are emissive (sparks, stun, glints).

| Kind | Effects |
|---|---|
| Atmosphere | **marine snow**: slow specks that are visible only near light. It is the most important effect, because it shows the light falloff. Vent sparks and smoke. Mineral specks that move with the grid drift. |
| Actions | flagellum wake, toxin trail, pilus shot and reel, injector stun sparks |
| Events | pickup glint, UV burn sparkles, engulf debris |

### Phases

1. Lightmap, darkness overlay with bands, level ambient, debug key, smoke-test
   screenshot.
2. Emissive masks from the generator, `DitherGlow`, glow data for each NPC
   kind, bloom.
3. Marine snow (`bevy_enoki`). Vent smoke and sparkles are done.
4. Action and event effects.
5. Pigment glimmer and tint, ambient for shallow water and land.

## Metabolism view

The Manage screen shows a cross-section of the cell (see `metabolism-view.md`).
Its art follows the same rules as above: 5-tone ramps, light from the upper
left, a 1 px outline, no anti-aliasing, dithered glow. All files are in
`assets/sprites/`; the animated sheets are also in `assets/sprites/anim/` and
`manifest.json`.

### Pool icons (`mol_<pool>`, 10x10)

`Currency` has 13 variants, so there are 13 icons. The file name is the variant
in snake_case: `mol_atp`, `mol_reducing_power`, `mol_pyruvate`,
`mol_carbon_skeletons`, `mol_amino_acids`, `mol_nucleotides`, `mol_lipids`,
`mol_metals`, `mol_waste`, `mol_minerals`, `mol_pigments`, `mol_o2`, `mol_ros`.

| Pool | Ramp | Shape |
|---|---|---|
| ATP | `atp`, lime | a coin with a ring |
| ReducingPower | `reducing`, electric blue | a bolt |
| Pyruvate | `pyruvate`, muted aqua | a small tilted pill |
| CarbonSkeletons | `carbon` (rule 5) | the flake of `food_carbon` |
| AminoAcids | `amino`, sage | a block with a side group |
| Nucleotides | `nucleotide`, orchid | a hexagonal base ring with a hole |
| Lipids | `head` and `tail` (the bilayer colours) | a head with two tails |
| Metals | `metal` (rule 5) | the cube of `pickup_metal_fes` |
| Waste | `waste`, murky olive-grey | a lumpy blob with dark flecks |
| Minerals | `mineral` (rule 5) | the shard of `pickup_mineral` |
| Pigments | `pigment` (the carotenoid orange) | a drop |
| O2 | `o2`, sea green (the colour of O2 on the grid) | a dumbbell of two atoms |
| ROS | `ros`, hot magenta | a spiky star. It is a hazard, and it does not use the red of perforin or the predator |

The new hues are kept away from the cell role colours (rule 4). Lime, electric
blue and sage are the nearest to the toxin green, the protein blue and the
symbiont gold, so the silhouette also carries the meaning. Gold-white is not
used: it stays for the gene reward (rule 5).

### Proteins, plates and icons

| File | Size | Notes |
|---|---|---|
| `enzyme` (`anim/enzyme_idle.png`) | 16x16, 8 frames, 8 fps | protein blue, a notch for the active site. Bobs by 1 px. Tint with the pathway accent at run time |
| `ribosome` (`anim/ribosome_loop.png`) | 24x24, 8 frames, 8 fps | a large and a small subunit (orchid, the small one a step lighter). The small one ratchets, the mRNA thread slides left, and sage beads of chain grow out of the large subunit |
| `pathway_plate` | 24x24 | nine-slice, **border 8 px**. The middle of every edge is the same along its length, so it stretches without a seam |
| `pathway_plate_ghost` | 24x24 | the same slices, dithered (ordered 4x4, folded so that the stretch stays clean). For a pathway the cell does not have yet |
| `pathway_icon_<block>` | 16x16 | one for each of the 13 `BlockKind`s with a `block_spec`: `chemosynthesis`, `light_capture`, `respiration`, `catalase`, `cuticle`, `amino_acid_biosynthesis`, `lipid_metabolism`, `protein_synthesis`, `fermentation`, `vesicle_export`, `fat_storage`, `nucleotide_cofactor_synthesis`, `secondary_metabolites` |
| `slot_socket` | 24x26 | an empty socket: a bilayer cut with a protein-blue collar round a dark pit. Tiles like `wall_membrane` (the bilayer is 24 px wide here) |
| `badge_limit` | 12x12 | a bar over a down arrow in a steel disc: the pathway is at its cap |
| `valve_off`, `valve_low`, `valve_high` | 12x12 | a pipe with a gate: shut, nearly shut, lifted out (with a mint flow mark) |

The plate, the badge and the valves are steel (`lytic`) on dark rock, so the
screen can tint them. Previews of the new sprites are in
`docs/art/contact_sheet.png`.

## Not done yet

- In the game: death, contact and hit clips, the other overlays, NPC cells, the
  engulf pairs (their manifest entries have a frame size for each clip, so the
  loader skips them for now), the vent backdrop and the walls.

- Engulf for other pairs (NPC predators eating prey), and a failed engulf (the
  target escapes, or is too big).
- The next stage's gate cell victory (shallow water, land).
- Contact with a corner, or with two walls. The squash is for one flat wall.
- Animation for the pickups (a twinkle) and for the vent (a pulse).
- A speed-dependent stretch for move (now there is one amount).
- Contact and hit frames for the virus.
- Wall pieces that turn corners, and a vertical wall.
- Wall pores for carbon and metal (now the wall pore is mineral-blue only, and
  does not show the substrate). Specific receptors (light, chemical) and an
  alternative look for a protein that is broken or switched off.
- Check the overlays on NPC cells. They use the same sprites.
- More immune parts: a free antibody (the projectile), cytokine and interferon
  alarm effects, the complement ring on a target cell, the killed-cell look
  after perforin.
- Shelter sprites (vent crevice, microbial mat, soil pore).
- M7 stand-ins: wall pieces for a microbial mat and a tough wall (now `wall_membrane` with a colour tint), an O2 colour on the grid (sea green, dithered), a "stressed" look for a cell with ROS (now a flat colour tint), a victory clip for the proteobacterium gate.
- Grid channel visuals: heat glow and mineral plume are in. Light and UV are in
  as stand-ins (pale yellow `#fff4b4` with a violet edge `#7c52cc`, dithered).
  The art owner must pick the real light ramp and rays. Also the gene orb in
  the game is still the plain sheet with no glow.
- Scene light, fluorescence, pigment glimmer and particles: see "Light and
  darkness" (a plan, nothing in the game yet). The art owner must pick the
  pigment colour, the warm and cool light tints and the number of light bands.
- UI frame, icons for the 4 action slots, HUD bars.
- Shallow-water and land palettes. The water backdrop and the rock ramp are for
  the vent stage only.
- M8 stand-ins (land): the ground is a flat dark earth colour (`FieldKind::background`,
  `#2a2017`); water on the grid is a dithered blue (`#1d5aa8` edge, `#7ac4f4` core);
  the waxy wall is `wall_membrane` with a yellow-green tint; the moss crust (a mat)
  and the tough wall keep their M7 tints. **Land palette for the art owner:** an
  earth ramp for the ground (the `rock` ramp warmed to brown), a leaf green and an
  ochre for plant-like tissue (waxy walls, the gate room), a pale blue for water and
  puddles. Creatures reuse the vent sheets. The land gate cell (a multicellular
  colony, `Multicellularity`) reuses `gate_cell_phototroph`: it should become a
  cluster of three or four small cells in one halo, with no victory clip of its own yet.
  A cell with low hydration needs a look too (now only the HUD bar).

## Missing art for the Codex screen

The Codex screen (`screens::codex`) shows a plain labelled tile where an entry
has no art. The list below is what is missing now. A pathway icon is
`pathway_icon_<gene>.png` (see "Proteins, plates and icons"); add it to
`PATHWAY_ICONS` in `src/codex_catalog.rs`.

- Pathway icons for genes with no block yet: `SugarCatabolism`,
  `OrganicAcidOxidation`, `NitrogenSulfurAssimilation`,
  `AromaticPrecursorSynthesis`, `Polymerization`, `Multicellularity`.
- A card frame for each badge (unknown, in codex, expressed, mutated, silent) and a
  "?" silhouette. The cards use a flat colour and a black tint now.
- A still image for each membrane protein. The cards use frame 0 of the `idle`
  clip of its overlay sheet (the east direction).

## The genome view (Codex)

The Codex shows the genome of the cell, not a grid of cards. Every ability
comes from a gene, so every catalog entry is a locus on one circular
chromosome. The chromosome is folded into rows, like the loop of a bacterial
nucleoid, and closes on a spine with `oriC`. `ter` is on a turn on the right.

- **Shader, not sprites.** `assets/shaders/genome.wgsl` draws the strand as a
  double helix from the segments of `genome_map::GenomeLayout`. Each fragment
  snaps to a sprite pixel, so the rules above hold at any zoom: hard tone bands,
  a 1 px outline, light on the crest of the front strand.
- **Colour means state.** Expressed loci use the player's mint ramp. Enhanced
  is gold, mutated is crimson (and kinked), silent is cold grey (a flatter,
  tighter helix with no base pairs). Genes that are only in the codex are a
  dashed pale-blue ghost. Unknown genes are nearly black. Membrane protein
  genes are teal. Linker DNA is grey violet.
- **Glow is dithered.** A locus whose product exists (enzymes or proteins)
  has a dithered halo that breathes.
- **Damage shows.** As `GenomeIntegrity` falls, the strand gets single-strand
  nicks and then breaks of both strands.
- **It moves a little.** Two slow waves travel along the strand and the helix
  turns in place. Icons and gene symbols move with the strand.
- **Icons are 1:1.** Pathway icons are 16 x 16. Membrane proteins use frame 0
  of the north-pointing row of their overlay sheet, cropped to 24 x 32, so the
  product grows up out of its gene.

Next step: **nucleosomes.** A later gene (histones) can wrap the linker DNA
round beads. This is the first step of genome protection against UV: the
shader can draw the beads on linkers, and `genome_damage` can lower the UV
damage to the genome.

## Light on the metabolic map

The Manage map uses the same rules as the genome view: pixel-snapped shaders,
tone bands, a 1 px dark outline, and dithered glow.

- **Cytoplasm** (`assets/shaders/cytoplasm.wgsl`, `screens::manage::light`).
  One quad behind the map, from the membrane down to the genome. Its light is
  ambient, plus light through the membrane (warm with outside heat, cool with
  outside light), plus emitters. A pathway that runs is an emitter in its own
  colour, and it breathes faster when it runs harder. A full pool glows a
  little. The light is cut into five bands of the cytoplasm ramp, with a
  dithered edge. The ambient light sits in the middle of a band, so calm
  cytoplasm is flat and the dither shows only where the light changes. Slow
  noise streams through the light, and specks drift with it.
- **Pipes** (`assets/shaders/pipe.wgsl`). One material per pipe and a quad per
  segment. A pipe is a lit tube: a dark wall, a core in the pipe colour, and a
  light line on the upper or left side. Packets move at the speed of the
  molecule icons. A pipe that limits its pathway shows a red pulse that runs
  backwards. A cofactor is a dotted pin. A still pipe is dark, not see-through.
- **Membrane band.** The bilayer tiles ripple one pixel in a slow wave.

