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
- **Headless build**: `cargo build --features headless` (no graphics/audio)

### Testing
- **Run all tests**: `cargo test`
- **Run specific test**: `cargo test <test_name>`
- **Headless tests**: `cargo test --features headless`
- **Property-based tests**: Uses `proptest` for invariant testing (see tests/currency_invariants.rs)
- **Test categories**: Integration tests, unit tests, metabolic flow tests, genome tests, property tests

### Feature Flags
- `full` (default): Complete build with graphics, audio, and UI
- `headless`: Minimal build for testing/CI (no graphics, audio, UI)
- `dev`: Development features with dynamic linking and dev tools
- `dev_native`: Development features plus file watching for hot reload

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