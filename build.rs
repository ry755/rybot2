use anyhow::Result;
use vergen::{vergen, Config};
use vergen::{ShaKind, TimestampKind, TimeZone};

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=main.rs");

    let mut config = Config::default();
    *config.build_mut().kind_mut() = TimestampKind::All;
    *config.build_mut().timezone_mut() = TimeZone::Local;
    *config.git_mut().sha_kind_mut() = ShaKind::Short;
    vergen(config)
}