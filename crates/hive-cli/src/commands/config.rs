use anyhow::{Result, bail};
use hive_core::config;
use hive_core::storage::HivePaths;

pub fn run(show: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let paths = HivePaths::new(&cwd);

    if !paths.hive_dir().exists() {
        bail!("not a hive project (missing .hive/ directory). Run `hive init` first");
    }

    if show {
        let entries = config::show_config(&paths.hive_dir())?;
        for (key, value, source) in entries {
            println!("{key}: {value}  ({source})");
        }
    } else {
        println!("use --show to display merged configuration");
    }

    Ok(())
}
