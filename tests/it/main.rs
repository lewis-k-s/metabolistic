//! All integration tests in one binary: one link step instead of one per file.
//! The `tui_*` tests stay separate binaries because they need the `tui` feature.

mod common;
mod build_regulate_test;
mod currency_invariants;
mod currency_test;
mod fat_storage_test;
mod fermentation_test;
mod flux_profile_test;
mod genome_damage_test;
mod genome_test;
mod integration_test;
mod land_test;
mod living_world_test;
mod metabolic_flow_test;
mod metabolic_invariants;
mod metabolism_view_test;
mod multi_system_interactions;
mod npc_benchmark;
mod precision_tests;
mod run_structure_test;
mod shallow_water_test;
mod simulation_core_test;
mod solver_invariants;
mod temporal_consistency;
mod vent_field_test;
mod vesicle_export_test;
