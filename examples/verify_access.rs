//! Read-only service reachability checks through an explicitly supplied proxy.
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let proxy = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("Pass an HTTP/SOCKS proxy URL"))?;
    if let clash_verge_tui::extras::ExtraResult::Detected(results) =
        clash_verge_tui::extras::execute(clash_verge_tui::extras::ExtraCommand::Detect {
            proxy,
            indices: (0..8).collect(),
        })
        .await?
    {
        for (_, row) in results {
            println!("{}: {} ({})", row.name, row.result, row.region);
        }
    }
    Ok(())
}
