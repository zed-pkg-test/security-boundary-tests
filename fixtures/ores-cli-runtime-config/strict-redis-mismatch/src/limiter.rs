use std::collections::HashMap;
use std::sync::Mutex;

static LIMITERS: Mutex<HashMap<String, u64>> = Mutex::new(HashMap::new());
const LEGACY_ENV: &str = "ZED_RATE_LIMIT_PER_SECOND";
