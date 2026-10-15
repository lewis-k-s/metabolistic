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
   things that matter: the vent heat, the symbiont, the gate cell.
8. **Walls are cross-sections.** A wall piece shows the bilayer cut open, as a
   band. This is the usual way to draw a wall from above, and it shows what the
   wall is made of.
9. **Body deform is baked into frames.** The cells animate with frames, not with
   a transform scale (see "Animation"). There is no soft-body physics: the
   deform is visual only.

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
- UI frame, icons for the 4 action slots, HUD bars.
- Shallow-water and land palettes. The water backdrop and the rock ramp are for
  the vent stage only.
