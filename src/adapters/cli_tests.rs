use super::config_level;

#[test]
fn config_level_parses_valid_level() {
    assert_eq!(config_level("debug"), tracing::Level::DEBUG);
}

#[test]
fn config_level_falls_back_to_info_on_invalid() {
    assert_eq!(config_level("chatty"), tracing::Level::INFO);
}
