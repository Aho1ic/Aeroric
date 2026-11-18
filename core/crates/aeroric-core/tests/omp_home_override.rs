//! B5-2 (2026-11-18): once the host configures the omp home, every omp path in
//! the process (settings, model discovery, upgrade env) is that home; before,
//! the legacy `HOME/.aeroric/agent-homes/omp`. Its own test binary: the
//! override is process-wide.

use aeroric_core::settings;

#[test]
fn the_configured_omp_home_wins_over_the_legacy_root() {
    // A scratch HOME: the legacy root is only computed, never the real one.
    let scratch =
        std::env::temp_dir().join(format!("aeroric-omp-scratch-{}", uuid::Uuid::new_v4()));
    std::env::set_var("HOME", &scratch);
    std::env::remove_var("AERORIC_HOME");
    let before = settings::managed_omp_home().expect("a home directory");
    assert_eq!(before, scratch.join(".aeroric/agent-homes/omp"));
    let data = std::env::temp_dir().join(format!("aeroric-omp-home-{}", uuid::Uuid::new_v4()));
    let home = data.join("agent-homes").join("omp");
    settings::configure_omp_home(&home);
    assert_eq!(settings::managed_omp_home(), Some(home.clone()));
    // The settings panel's built-in config path follows it too.
    assert_eq!(
        settings::builtin::default_builtin_agent_config_path("omp").unwrap(),
        home.join("config.yml")
    );
    assert!(!data.exists(), "configuring writes nothing");
    assert!(!scratch.exists(), "nor does reading the legacy root");
}
