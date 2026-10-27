#[cfg(windows)]
mod app;

#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    app::main()
}

#[cfg(not(windows))]
fn main() -> anyhow::Result<()> {
    anyhow::bail!("memory_tool requires Windows")
}
