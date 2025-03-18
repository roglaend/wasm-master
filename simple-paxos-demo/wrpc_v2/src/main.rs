use proposer_wrpc_v2::run_proposer_wrpc_exports;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr = "[::1]:7761";
    let _ = run_proposer_wrpc_exports(addr).await?;
    wrpc_transport::Serve::serve(&self, instance, func, paths)
    Ok(())
}
