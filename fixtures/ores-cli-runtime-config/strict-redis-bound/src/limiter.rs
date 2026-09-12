const CONFIG: &str = include_str!("../.ores-rl.toml");
const REDIS_URL_ENV: &str = "REDIS_URL";

fn boot() {
    ores_rl::admit_runtime(CONFIG, REDIS_URL_ENV);
}
