use muzak_player::AppWindow;
use slint::ComponentHandle;

fn main() -> anyhow::Result<()> {
    let window = AppWindow::new()?;
    window.run()?;
    Ok(())
}
