const CONFIG: &str = include_str!("../.ores-mw.toml");

fn boot() {
    admit_server_stack(CONFIG);
}
