//! Fresh OS elevation is required per invocation. No listener/daemon/shell IPC.
fn main() {
    if let Err(error) = run() {
        eprintln!("{}", serde_json::json!({"error":error}));
        std::process::exit(1);
    }
}
fn run() -> devforge_serving::Result<()> {
    use devforge_serving::{ServingError as E, system::Plan};
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 || args[0] != "apply" {
        return Err(E::InvalidConfiguration);
    }
    let bytes = std::fs::read(&args[1])?;
    if bytes.len() > 128 * 1024 {
        return Err(E::InvalidConfiguration);
    }
    let plan: Plan = serde_json::from_slice(&bytes).map_err(|_| E::InvalidConfiguration)?;
    let effect = plan.apply(&args[2])?;
    println!("{}", serde_json::to_string(&effect).unwrap());
    Ok(())
}
