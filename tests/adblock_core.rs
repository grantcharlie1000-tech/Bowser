// These production modules are also executable on Linux, independently of the Windows host.
// Run: rustc --edition=2024 --test tests/adblock_core.rs -o /tmp/bowser-adblock-core
#[path = "../src/blocker/json.rs"]
mod json;
#[path = "../src/blocker/request_url.rs"]
mod request_url;
use request_url::split_url;
#[path = "../src/blocker/response_lifecycle.rs"]
mod response_lifecycle;
#[path = "../src/blocker/youtube.rs"]
mod youtube;

#[path = "../src/blocker/response.rs"]
mod response;
