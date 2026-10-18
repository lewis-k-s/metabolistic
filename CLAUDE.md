# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Metabolistic3D is a 3D cellular metabolism simulation game built with the Bevy engine. It simulates cellular biochemistry through interconnected metabolic blocks that mirror real biological processes, teaching players about metabolism through engaging gameplay.

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
- `pilus.rs`, `stun.rs`: the type IV pilus (shoot, hold to reel, release) and the type VI injector (contact stun). Pure rules, headless. `world/pilus_view.rs` adds the avian2d joint and the line sprite. The aim of `ActionState` follows the cursor on the Run screen. Try them with `METABOLISTIC_SCENARIO=vent_predator cargo run` (pilus on the left mouse button, injector on the right). Bindings on the Run screen: WASD moves, Shift is the flagellum burst, the four action slots are the left, right, back and forward mouse buttons (keys 1-4 also work). The flagellum trails the move direction (`ActionState::heading`); aimed proteins point at the cursor (`aim`)
- `lighting.rs`: the pure light field (ambient + heat + shaft light) and the lightmap; `world/lighting_view.rs` draws it as a banded, dithered darkness overlay (`assets/shaders/lighting.wgsl`). Plan: `docs/art-direction.md`, "Light and darkness"
- `movement.rs`: pure thrust, mass, drag and ATP-cost formulas
- `level.rs`: the hand-built vent field
- `run.rs`: death detection and `RunSummary`
- `world/`: the run world (full only): grid texture, vents, player body (avian2d), camera, HUD
- `metabolism/`: Core metabolic flow simulation and graph management
- `codex_catalog.rs`: the catalog of all genes and membrane proteins (`CodexEntry`), pure state and edit rules. A new `BlockKind` or `MembraneProtein` needs one arm in `gene_facts` or `protein_facts`
- `screens/`: One module per screen (`loading`, `menu`, `run`, `manage`, `run_over`), the Codex overlay (`codex`, key C in Manage), the shared 2D camera, and UI helpers
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

### Smoke Test (full game)
- `METABOLISTIC_SMOKE_TEST=1 cargo run` opens the window for about 15 s, checks that `Loading` opens the menu, visits every screen and menu page, holds W on the Run screen, saves one screenshot per step to `target/smoke/` (or `METABOLISTIC_SMOKE_DIR`), logs the player cell's position and pools, and exits. A panic fails the run (non-zero exit), also if the player dies or does not move while W is held.
- Use it after engine upgrades or changes to screens, cameras, input or UI. It is the one allowed exception to "do not run `cargo run`" below, because it exits on its own.
- Expected noise: bevy_egui "Bindless textures are not yet supported on metal", and a winit "Skipped event Destroyed" on exit.

### Scenario Launch (fast testing)
- `METABOLISTIC_SCENARIO=vent_motile cargo run` (or `cargo run -- --scenario vent_motile`) opens directly in a Run with a prepared cell. It skips Menu and Prepare. A bare name is `assets/scenarios/<name>.json`; a name with `/` or `.json` is a path. A bad file stops the game with the parse error.
- A scenario file sets stage, seed, spawn, genes, enzymes, membrane proteins, action slots, pools, codex genes and `invulnerable`. All fields are optional (`{"stage":"Vent","membrane":{"Flagellum":3}}` is valid). Format and rules: the doc comment of `src/scenario.rs`.
- Presets: `vent_motile` (3 flagella, transporter, burst), `vent_hunter` (flagella, adhesin, toxin, high ATP), `shallow_start` (light capture, catalase), `land_start`. To add one, put a JSON file in `assets/scenarios/`; the test `every_preset_loads_and_builds_a_cell` checks it.
- F9 in a run writes the cell's current state as a scenario to `scenario-dump.json` in the save dir. A scenario run never writes the real codex file.
- The scenario launch does not work together with `METABOLISTIC_SMOKE_TEST`.

### Testing
- **Run all tests**: `cargo test` (full build, about 10 s after an edit)
- **Headless tests**: `cargo th` (alias for `cargo test --no-default-features --features headless`). Use this loop for simulation work.
- **Run specific test**: `cargo test <test_name>`
- **Integration test layout**: all integration tests are modules of one binary, `tests/it/main.rs` (one link step instead of one per file). Add a new test file to `tests/it/` and a `mod` line to `main.rs`. Only the `tui_*` tests stay separate binaries. Run one file with `cargo test --test it <module>::`. Proptest seeds are in `tests/proptest-regressions/`.
- **Simulation tests**: use `MetabolisticApp::new_headless()` (manual ticks, no random mutations). Advance with `app.update()` (genome -> block sync) and `metabolism::run_metabolic_tick(app.world_mut())`. `tests/it/common/mod.rs` has `sim_step` and `express_blocks`.
- **TUI tests**: `cargo test --features tui` (the `tui_*` tests need the `tui` feature)
- **Property-based tests**: Uses `proptest` for invariant testing. Shared strategies are in `tests/it/common/mod.rs`. Do not set `timeout` in `ProptestConfig`: it forks one process per case and makes tests very slow.
- **Test categories**: Integration tests, unit tests, metabolic flow tests, genome tests, property tests

### Feature Flags
- `full` (default): Complete build with graphics, audio, UI, and egui inspector
- `headless`: Simulation only (no graphics, audio, UI, physics). Always use with `--no-default-features`.
- `tui`: Opt-in terminal monitor for cell state. Not in `full`, because it takes over the terminal. It turns itself off when stdout is not a terminal.
- `dev`: Development features with dynamic linking and dev tools
- `dev_native`: Development features plus file watching for hot reload

Graphical modules (`screens`, `dev_tools`, `debug`, `inspector`, `smoke_test`) compile only with `full`. Simulation modules (`blocks`, `cell`, `metabolism`, `molecules`, `shared`) must not depend on them.

### Development Tools (In-Game)
- **Inspector**: F1 to open runtime debugging and inspection tools
- **UI Debug**: `` ` `` (backquote) toggles UI node outlines
- **Lighting**: `F6` turns the darkness overlay on and off (Run screen). The light is `lighting.rs` (pure) and `world/lighting_view.rs`
- **Screen Navigation**: `Tab` switches Run and Manage; `Esc` returns to the menu

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
- **Do not run `cargo run`** in development environments - it will launch the full UI
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