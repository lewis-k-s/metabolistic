# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Metabolistic3D is a 3D cellular metabolism simulation game built with the Bevy engine. It simulates cellular biochemistry through interconnected metabolic blocks that mirror real biological processes, teaching players about metabolism through engaging gameplay.

## Architecture

### Core Structure
- **Library-based architecture**: Main logic in `src/lib.rs` with binary entry point in `src/main.rs`
- **Dual-app pattern**: `MetabolisticApp::new()` for full game, `MetabolisticApp::new_headless()` for testing
- **Plugin-based system**: Uses Bevy's plugin architecture for modular game systems
- **State management**: Implements game states (MainMenu, Scene3D, Scene2D, GenomeEditing) with proper transitions
- **ECS pattern**: Leverages Bevy's Entity-Component-System for game logic

### Key Modules
- `blocks/`: Metabolic pathway implementations (fermentation, fat_storage, vesicle_export, genome)
- `molecules.rs`: Currency system for ATP, NADH/NADPH, and metabolic precursors
- `metabolism/`: Core metabolic flow simulation and graph management
- `scenes/`: Different game views (3D world, 2D flowmap, genome editor, menu)
- `player/`: Player controller and movement systems
- `camera.rs`: Camera management across different scenes
- `shared.rs`: Common resources and state transition systems

### Plugin Architecture
The app uses a plugin-based architecture where each major system is implemented as a Bevy plugin:
- `CurrencyPlugin`: Manages ATP, NADH, and other metabolic currencies
- `GenomePlugin`: Handles genome editing and visualization
- `FermentationPlugin`: Implements fermentation metabolic pathways
- `FatStoragePlugin`: Manages lipid storage and fatty acid metabolism
- `VesicleExportPlugin`: Handles cellular export mechanisms
- `MetabolicFlowPlugin`: Core metabolic simulation engine and graph management
- Scene-specific plugins for different game states

## Development Commands

### Build Commands
- **Development build**: `cargo run` (includes hot reloading, debug tools)
- **Optimized build**: `cargo run --profile release-native`
- **Web-optimized build**: `cargo build --release`
- **Headless build**: `cargo build --no-default-features --features headless` (no graphics/audio)
- **Headless check**: `cargo ch` (alias, see `.cargo/config.toml`)

### Testing
- **Run all tests**: `cargo test` (full build, about 10 s after an edit)
- **Headless tests**: `cargo th` (alias for `cargo test --no-default-features --features headless`). Use this loop for simulation work.
- **Run specific test**: `cargo test <test_name>`
- **Quarantined tests**: `cargo tq` runs the tests marked `#[ignore = "pre-pivot Cell model: ..."]`. They fail because `Cell` holds `CurrencyPools` as a field, not as a component. Remove the `#[ignore]` when the Cell model is fixed.
- **TUI tests**: `cargo test --features tui` (the `tui_*` tests need the `tui` feature)
- **Property-based tests**: Uses `proptest` for invariant testing. Shared strategies are in `tests/common/mod.rs`. Do not set `timeout` in `ProptestConfig`: it forks one process per case and makes tests very slow.
- **Test categories**: Integration tests, unit tests, metabolic flow tests, genome tests, property tests

### Feature Flags
- `full` (default): Complete build with graphics, audio, physics, and egui inspector
- `headless`: Simulation only (no graphics, audio, UI, physics). Always use with `--no-default-features`.
- `tui`: Opt-in terminal monitor for cell state. Not in `full`, because it takes over the terminal. It turns itself off when stdout is not a terminal.
- `dev`: Development features with dynamic linking and dev tools
- `dev_native`: Development features plus file watching for hot reload

Graphical modules (`camera`, `scenes`, `dev_tools`, `debug`, `inspector`, and the physics player in `player`) compile only with `full`. Simulation modules (`blocks`, `cell`, `metabolism`, `molecules`, `shared`) must not depend on them.

### Development Tools (In-Game)
- **Inspector**: F12 to open runtime debugging and inspection tools
- **Physics Debug**: Enabled by default in development builds
- **State Transitions**: Press number keys to switch between game states
- **Genome Editor**: Press `3` to open circular genome visualization

## Code Patterns

### Component Architecture
- Use `#[derive(Component)]` for game entities
- Implement required components with `#[require(...)]` attribute
- Prefer component composition over inheritance

### System Organization
- Systems are organized by functionality in plugin modules
- Use `Startup` schedule for initialization
- Use `Update` schedule for per-frame logic
- State-specific systems use run conditions like `.run_if(in_state(GameState::Scene3D))`

### ECS Query Patterns (PREFERRED)
When working with cell data, prefer ECS query patterns over world-querying:

**Preferred Pattern:**
```rust
fn update_cell_currencies(
    mut cell_query: Query<&mut CurrencyPools, With<Cell>>,
    player_query: Query<&ControlledCell, With<LocalPlayer>>,
) {
    if let Ok(controlled_cell) = player_query.get_single() {
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
- MetabolicGraph and FluxResult resources track metabolic calculations
- FlowDirty resource triggers metabolic graph rebuilds when set to true

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
- **Integration tests**: Cross-plugin interactions (tests/integration_test.rs)
- **Property-based tests**: Invariant testing using proptest (tests/*_invariants.rs)
- **Metabolic tests**: Currency flows, genome manipulation, fermentation (tests/metabolic_*.rs)
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