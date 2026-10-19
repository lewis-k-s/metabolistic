# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Metabolistic3D is a 3D cellular metabolism simulation game built with the Bevy engine. It simulates cellular biochemistry through interconnected metabolic blocks that mirror real biological processes, teaching players about metabolism through engaging gameplay.

## Agents: the Game Runs Offscreen (no game windows)

- **When an agent starts the game, it opens no window.** `src/offscreen.rs` turns on offscreen mode when it finds `CLAUDECODE`, `AI_AGENT`, `CODEX_SANDBOX` or `CODEX_THREAD_ID`. This applies to `cargo run`, the smoke test, scenario launches and the playtest. Several sessions can run the game at the same time, and it works with the screen locked (tested 2026-10-18).
- `METABOLISTIC_OFFSCREEN=1` forces the mode on, and `METABOLISTIC_OFFSCREEN=0` forces a window. **Do not set `METABOLISTIC_OFFSCREEN=0` unless the user asks to see a window.**
- How it works: winit is off, so the primary `Window` entity has no OS window (fixed 1280x720; size queries work, the cursor position is `None`). Each camera that targets the primary window renders to one image (`OffscreenTarget`), and each `Screenshot::primary_window()` captures that image. Game code needs no changes. Use the Read tool to look at the PNGs.
- A launch with no script (no smoke test, no playtest) saves one frame every 2 s to `METABOLISTIC_OFFSCREEN_DIR` (default `target/offscreen`) and exits after `METABOLISTIC_OFFSCREEN_SECS` (default 20). Example: `METABOLISTIC_SCENARIO=vent_motile METABOLISTIC_OFFSCREEN_SECS=8 cargo run`.
- For long unattended runs, put `caffeinate -i` before the command, so that the Mac does not sleep.
- Not done yet: there is no virtual cursor (aim and mouse hover do nothing), and time is real time, not manual steps. `cargo run --example offscreen_spike` (`-- --soak 180`) is the minimal pattern; its soak run logs the screen lock state for each frame.

## Architecture

### Core Structure
- **Library-based architecture**: Main logic in `src/lib.rs` with binary entry point in `src/main.rs`
- **Dual-app pattern**: `MetabolisticApp::new()` for full game, `MetabolisticApp::new_headless()` for testing
- **Plugin-based system**: Uses Bevy's plugin architecture for modular game systems
- **Screens**: `GameState` has one variant per screen: `Loading` -> `Menu` -> `Run` / `Manage` -> `RunOver`. The game is 2D only. The computed state `InRun` covers Run and Manage: the world (grid and player cell) lives exactly while `InRun` is active.
- **ECS pattern**: Leverages Bevy's Entity-Component-System for game logic

### Key Modules
- `blocks/`: Metabolic pathway implementations (fermentation, fat_storage, vesicle_export, genome)
- `molecules.rs`: Currency system for the 9 internal pools
- `envgrid.rs`: `EnvGrid`, the spatial grid (Heat, Minerals): pure diffusion, drift, source and decay steps, sampling
- `exchange.rs`: grid <-> cell exchange (mineral uptake, heat damage, movement ATP cost) in the metabolic tick
- `pilus.rs`, `stun.rs`: the type IV pilus (shoot, hold, scroll down reels in and scroll up pays out, release) and the type VI injector (contact stun). Pure rules, headless. `world/pilus_view.rs` adds the avian2d joint (needs avian's `xpbd_joints` feature) and the line sprite. The aim of `ActionState` follows the cursor on the Run screen. Try them with `METABOLISTIC_SCENARIO=vent_predator cargo run` (pilus on the left mouse button, injector on the right). Bindings on the Run screen: WASD moves, Shift is the flagellum burst, the four action slots are the left, right, back and forward mouse buttons (keys 1-4 also work), and Q and E toggle the two secretions (`secretion::SecretionBar`). The flagellum trails the move direction (`ActionState::heading`); aimed proteins sit in a cluster round the cursor (`aim`): the last used one (`AimOrder`) points exactly at it, the others one row to each side
- `secretion/`: area clouds that a cell secretes, with no aim (plan: `docs/secretion-abilities-plan.md`). Each is a gene with a block whose `FluxProfile` has demands only; its `Drive` is 0 until Q or E switches it on, and its turnover goes into an `EnvGrid` channel (`Channel::SECRETED`). `microcin` (`cvaC`, hurts prey), `slime` (`pelA`, drag on other cells), `siderophore` (`entF`, dissolves `particles::RustPickup` into Metals), `autoinducer` (`luxS`, AI-2: symbionts clump, predators listen). Empty channels go dormant and `EnvGrid::step` skips them. `world/cloud_view.rs` draws the clouds (`assets/shaders/clouds.wgsl`)
- `lighting.rs`: the pure light field (ambient + heat + shaft light) and the lightmap; `world/lighting_view.rs` draws it as a banded, dithered darkness overlay (`assets/shaders/lighting.wgsl`). Plan: `docs/art-direction.md`, "Light and darkness"
- `movement.rs`: pure thrust, mass, drag and ATP-cost formulas
- `level.rs`: the hand-built vent field
- `run.rs`: death detection and `RunSummary`
- `world/`: the run world (full only): grid texture (O2, water, land minerals; heat, light and vent minerals are drawn by the lighting), sea floor, vents, `vfx` (vent smoke and sparkles, `bevy_enoki`), player body (avian2d), camera, HUD
- `metabolism/`: Core metabolic flow simulation and graph management
- `codex_catalog.rs`: the catalog of all genes and membrane proteins (`CodexEntry`), pure state and edit rules. A new `BlockKind` or `MembraneProtein` needs one arm in `gene_facts` or `protein_facts`, and a real gene symbol in `gene_symbol`
- `genome_map.rs`: the pure layout of the Codex genome: a circular chromosome folded into rows (`GenomeLayout`), one locus per catalog entry, the wobble and helix formulas, the hit test (`pick`) and the colour of a locus (`Look`). `assets/shaders/genome.wgsl` uses the same geometry
- `screens/`: One module per screen (`loading`, `menu`, `run`, `manage`, `run_over`), the Codex overlay (`codex`, key C in Manage), the shared 2D camera, the Run <-> Manage zoom (`zoom`), and UI helpers. The Codex is the genome of the cell: `codex/genome_view.rs` draws it below the Manage map and pans the map camera to it (on the menu and Prepare it brings its own camera). While the Codex is open it hides the other screens' root UI nodes
- `player/`: The player-cell link (`LocalPlayer`, `ControlledCell`) and spawn helpers
- `shared.rs`: Starter cell and screen navigation (`navigate`, a pure function)
- `sprites.rs` (`full` only): loads the sheets listed in `assets/sprites/anim/manifest.json` (`Sheets`) and plays them (`Animator`). The art comes from `art/gen_sprites.py`; see `docs/art-direction.md`

### Plugin Architecture
The app uses a plugin-based architecture where each major system is implemented as a Bevy plugin:
- `CurrencyPlugin`: Manages ATP, NADH, and other metabolic currencies
- `GenomePlugin`: Handles genome editing and visualization
- `FermentationPlugin`: Implements fermentation metabolic pathways
- `FatStoragePlugin`: Manages lipid storage and fatty acid metabolism
- `VesicleExportPlugin`: Handles cellular export mechanisms
- `MetabolicFlowPlugin`: Core metabolic simulation engine and graph management
- `screens::plugin`: the four screens. Each screen tags its entities with `DespawnOnExit(GameState::X)`; menu pages use the `MenuPage` sub-state the same way

## Development Commands

### Build Commands
- **Toolchain**: Rust 1.96.1, pinned in `rust-toolchain.toml` (Bevy 0.19 needs 1.95+). Bevy 0.20 exists, but bevy-inspector-egui does not support it yet.
- **Development build**: `cargo run` (includes hot reloading, debug tools)
- **Optimized build**: `cargo run --profile release-native`
- **Web-optimized build**: `cargo build --release`
- **Headless build**: `cargo build --no-default-features --features headless` (no graphics/audio)
- **Headless check**: `cargo ch` (alias, see `.cargo/config.toml`)
- **New worktree**: `scripts/seed-target` clones the main checkout's `target/debug` (APFS clone, about 40 s, no disk space) so that the first build does not compile Bevy again (4-8 min). The `SessionStart` hook in `.claude/settings.json` runs it in the background for Claude worktrees; run it by hand in a Codex worktree before the first build.
- **Debug info**: our crate builds with `debug = "line-tables-only"` (backtraces have lines; no variables in a debugger). Set `debug = true` locally to debug.

### Smoke Test (full game)
- `METABOLISTIC_SMOKE_TEST=1 cargo run` runs for about 60 s (offscreen for agents, a window otherwise), checks that `Loading` opens the menu, visits every screen and menu page, holds W on the Run screen, saves one screenshot per step to `target/smoke/` (or `METABOLISTIC_SMOKE_DIR`), logs the player cell's position and pools, and exits. A panic fails the run (non-zero exit), also if the player dies or does not move while W is held.
- Use it after engine upgrades or changes to screens, cameras, input or UI. It is the one allowed exception to "do not run `cargo run`" below, because it exits on its own.
- Expected noise: bevy_egui "Bindless textures are not yet supported on metal", and a winit "Skipped event Destroyed" on exit.

### Scenario Launch (fast testing)
- `METABOLISTIC_SCENARIO=vent_motile cargo run` (or `cargo run -- --scenario vent_motile`) opens directly in a Run with a prepared cell. It skips Menu and Prepare. A bare name is `assets/scenarios/<name>.json`; a name with `/` or `.json` is a path. A bad file stops the game with the parse error.
- A scenario file sets stage, seed, spawn, genes, enzymes, membrane proteins, action slots, pools, codex genes and `invulnerable`. All fields are optional (`{"stage":"Vent","membrane":{"Flagellum":3}}` is valid). Format and rules: the doc comment of `src/scenario.rs`.
- Presets: `vent_motile` (3 flagella, transporter, burst), `vent_hunter` (flagella, adhesin, toxin, high ATP), `shallow_start` (light capture, catalase), `land_start`, `vent_chemist` (microcin, slime, injector), `shallow_chemist` (siderophore, AI-2). To add one, put a JSON file in `assets/scenarios/`; the test `every_preset_loads_and_builds_a_cell` checks it.
- F9 in a run writes the cell's current state as a scenario to `scenario-dump.json` in the save dir. A scenario run never writes the real codex file.
- The scenario launch does not work together with `METABOLISTIC_SMOKE_TEST`.
- **Look gallery**: add `METABOLISTIC_LOOK_GALLERY=1` to a scenario launch to spawn one NPC of each body look round the player (`src/look_gallery.rs`); with the offscreen frames it is the review for the shader bodies. A prey dies after 4 s and the lysis shot goes to `target/gallery/lysis.png`.
- **Scripted playtest**: add `METABOLISTIC_PLAYTEST=1` to a scenario launch (`METABOLISTIC_SCENARIO=vent_chemist METABOLISTIC_PLAYTEST=1 cargo run`). It presses Q, E and W on a fixed script, logs pools, cloud levels and nearby NPCs each second, saves screenshots to `METABOLISTIC_PLAYTEST_DIR` (default `target/playtest`) and exits after 20 s (`src/playtest.rs`). Like the smoke test, it exits on its own. Use an `"invulnerable": true` scenario so the run does not end early.

### Testing
- **Run all tests**: `cargo test` (full build, 20-45 s after an edit). It does not build doctests, the binary's test harness or the example. For one area, `cargo test --test it <module>::` or `cargo test --lib <path>` is faster
- **Headless tests**: `cargo th` (alias for `cargo test --no-default-features --features headless`). Use this loop for simulation work.
- **Run specific test**: `cargo test <test_name>`
- **Integration test layout**: all integration tests are modules of one binary, `tests/it/main.rs` (one link step instead of one per file). Add a new test file to `tests/it/` and a `mod` line to `main.rs`. Only the `tui_*` tests stay separate binaries. Run one file with `cargo test --test it <module>::`. Proptest seeds are in `tests/proptest-regressions/`.
- **Simulation tests**: use `MetabolisticApp::new_headless()` (manual ticks, no random mutations). Advance with `app.update()` (genome -> block sync) and `metabolism::run_metabolic_tick(app.world_mut())`. `tests/it/common/mod.rs` has `sim_step` and `express_blocks`.
- **TUI tests**: `cargo test --features tui` (the `tui_*` tests need the `tui` feature)
- **Property-based tests**: Uses `proptest` for invariant testing. Shared strategies are in `tests/it/common/mod.rs`. Do not set `timeout` in `ProptestConfig`: it forks one process per case and makes tests very slow.
- **Test categories**: Integration tests, unit tests, metabolic flow tests, genome tests, property tests

### Feature Flags
- `full` (default): 2D graphics, UI, picking, physics and the egui inspector. It lists the Bevy parts one by one (not `bevy/default`): no 3D, audio, gamepads, scenes, UI widgets, sysinfo or `bevy_dev_tools`. If code needs one of these, add its Bevy feature to `full` in `Cargo.toml`
- `headless`: Simulation only (no graphics, audio, UI, physics). Always use with `--no-default-features`.
- `tui`: Opt-in terminal monitor for cell state. Not in `full`, because it takes over the terminal. It turns itself off when stdout is not a terminal.
- `dev`: Development features with dynamic linking and dev tools
- `dev_native`: Development features plus file watching for hot reload

Graphical modules (`screens`, `dev_tools`, `debug`, `inspector`, `offscreen`, `smoke_test`) compile only with `full`. Simulation modules (`blocks`, `cell`, `metabolism`, `molecules`, `shared`) must not depend on them.

### Development Tools (In-Game)
- **Inspector**: F1 to open runtime debugging and inspection tools
- **UI Debug**: `` ` `` (backquote) toggles UI node outlines
- **Lighting**: `F6` turns the darkness overlay on and off (Run screen). The light is `lighting.rs` (pure) and `world/lighting_view.rs`
- **Screen Navigation**: `Tab` zooms between Run and Manage; `Esc` returns to the menu

## Code Patterns

### Component Architecture
- Use `#[derive(Component)]` for game entities
- Implement required components with `#[require(...)]` attribute
- Prefer component composition over inheritance

### System Organization
- Systems are organized by functionality in plugin modules
- Use `Startup` schedule for initialization
- Use `Update` schedule for per-frame logic
- State-specific systems use run conditions like `.run_if(in_state(GameState::Run))`

### ECS Query Patterns (PREFERRED)
When working with cell data, prefer ECS query patterns over world-querying:

**Preferred Pattern:**
```rust
fn update_cell_currencies(
    mut cell_query: Query<&mut CurrencyPools, With<Cell>>,
    player_query: Query<&ControlledCell, With<LocalPlayer>>,
) {
    if let Ok(controlled_cell) = player_query.single() {
        if let Ok(mut currency_pools) = cell_query.get_mut(controlled_cell.0) {
            currency_pools.modify(Currency::ATP, 10.0);
        }
    }
}
```

**Anti-Pattern (avoid):**
```rust
fn update_cell_currencies(world: &mut World) {
    let cell_entity = find_cell_somehow(world);
    world.get_entity_mut(cell_entity).unwrap().get_mut::<CurrencyPools>().unwrap().modify(Currency::ATP, 10.0);
}
```

**Cell Wrapper Methods:** Cell wrapper methods should accept Query parameters instead of World references:
```rust
impl Cell {
    // Preferred
    pub fn get_currency(&self, currency_query: &Query<&CurrencyPools, With<Cell>>, currency: Currency) -> f32
    
    // Anti-pattern
    pub fn get_currency(&self, world: &World, currency: Currency) -> f32
}
```

### Resource Management
- Currency systems use shared resources accessible across plugins
- State transitions managed through `NextState<GameState>` resource
- Asset loading handled through Bevy's `AssetServer`

### Simulation Model
- **Cell**: an entity with the `Cell` marker. `#[require]` adds `Genome`, `CurrencyPools`, `CellBlocks`, and `MetabolicGraph`. There is no global `Genome` resource: each cell's `Genome` component is the only source of truth. UI code reads the local player's cell with `player::LocalCellGenome`.
- **Blocks**: one entity per implemented gene, with `MetabolicBlock`, `MetabolicNode` (kind, status), `FluxProfile` (rates per second), and `OwnedBy(cell)`. `blocks::sync_cell_blocks_system` keeps them in line with the genome (Expressed = full rate, Mutated = half, Silent = no flux). Removing a cell despawns its blocks.
- **Metabolic tick**: `MetabolicSchedule`, in the sets Genome -> Regulate -> Solve -> Exchange. `metabolism::solve_tick` is a pure function: each block runs at the largest activity in [0, 1] that its inputs allow, so stoichiometry is conserved and pools never go negative. All rates scale by `dt = FlowmapTimingConfig::target_delta()`.
- **Tick-length independence**: regulation and exchange rules must not overshoot a target by `rate * dt`. Cap at the gap (see `fat_storage_profile`) or use exact decay (`decay_fraction`). `time_scale_consistency` tests this.
- **New block kind**: add the gene to `BlockKind`, a profile to `blocks::block_flux_profile`, and a marker in `blocks::spawn_block`. Regulated blocks set their `FluxProfile` in `MetabolicSet::Regulate`.

### Error Handling
- Tests use `Result` types with proper error propagation
- Headless mode available for testing without graphics/audio dependencies
- Setup script (`codex-setup.sh`) handles environment dependencies
- Integration tests verify app startup and multi-frame operation

## Metabolic System Design

The game implements a sparse metabolic network with:
- **Currency Hub**: ATP, NADH/NADPH, Acetyl-CoA, Carbon Skeletons, ReducingPower
- **Metabolic Blocks**: Light Capture, Sugar Catabolism, Respiration, Fermentation, Fat Storage, Vesicle Export
- **Genome Control**: Blocks can be Silent, Active, or Mutated based on gene expression
- **Flow Calculation**: MetabolicGraph manages flux calculations and currency balancing
- **Minimal Cross-talk**: Only essential metabolites flow between blocks
- **Real Biochemistry**: Based on actual cellular metabolism pathways

## Testing Strategy

The codebase includes comprehensive testing with multiple categories:
- **Unit tests**: Individual components and systems (embedded in module files)
- **Integration tests**: Cross-plugin interactions (tests/it/integration_test.rs)
- **Property-based tests**: Invariant testing using proptest (tests/it/*_invariants.rs)
- **Metabolic tests**: Currency flows, genome manipulation, fermentation (tests/it/metabolic_*.rs)
- **Precision tests**: Numerical accuracy and temporal consistency
- **Headless testing**: All tests run without graphics dependencies using MetabolisticApp::new_headless()

## Development Warnings
- **Do not run `cargo run`** for a person's window. For an agent, `cargo run` is offscreen and exits by itself (see "Agents: the Game Runs Offscreen"); the smoke test and `cargo run --example offscreen_spike` also exit by themselves
- **Use headless mode** for testing: `MetabolisticApp::new_headless()` in tests
- **GPU/Graphics dependencies**: Full build requires graphics drivers; use `--features headless` for CI/containers
- **Audio dependencies**: Full build requires ALSA/audio libraries; use setup script for containers (`codex-setup.sh`)

# As you progress incrementally, run limited tests with tags on your specific block or feature

# After every major change, run the tests with `cargo test`

# If you need code documentation use context7

# Use `rg` instead of `grep` in general

# Run `cargo modules --lib` to see the up-to-date project structure

## Code Philosophy

- **Bevy Systems Methodology**:
  - Follow the rule: Bevy systems fetch data, pure functions mutate it. Try to keep a separation from the ECS queries and the logic for each step.