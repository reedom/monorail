mod error;
mod tracing_setup;

fn main() -> anyhow::Result<()> {
    tracing_setup::init();
    tracing::info!("monorail starting");
    Ok(())
}
