use anyhow::Context as _;
use std::convert::TryInto;
use std::sync::Arc;
use tokio::{net::TcpListener, pin, select, signal, task::JoinSet};
use wit_bindgen_wrpc::{
    bytes::Bytes,
    futures::{self, stream::SelectAll, StreamExt as _},
    tokio,
    tracing::{debug, error, info, warn},
    wrpc_transport::{self, ResourceBorrow, Server},
};

/// Converts a resource handle into a `usize` index.
///
/// The resource handle is expected to be the little‑endian encoding of a `usize`.
pub fn handle_to_index<T>(handle: ResourceBorrow<T>) -> anyhow::Result<usize> {
    let handle_bytes = Bytes::from(handle);
    let arr: [u8; std::mem::size_of::<usize>()] =
        handle_bytes.as_ref().try_into().context("invalid handle")?;
    Ok(usize::from_le_bytes(arr))
}

/// Define a type alias for the concrete wRPC Server type to simplify the generic bounds.
type MyServer = Server<
    std::net::SocketAddr,
    wit_bindgen_wrpc::tokio::net::tcp::OwnedReadHalf,
    wit_bindgen_wrpc::tokio::net::tcp::OwnedWriteHalf,
>;

/// A generic function that runs a component's export invocations on a given address.
///
/// # Type Parameters
///
/// - `ServeFut`: the future returned by `serve_fn`,
/// - `ServeFn`: the function/closure type that wires the exports,
/// - `Handler`: the component's handler type,
/// - `I`: the type for the invocation instance identifier,
/// - `N`: the type for the invocation name,
/// - `S`: the invocation stream type,
/// - `Fut`: the future type for each individual invocation.
///
/// The `serve_fn` closure is expected to return a vector of tuples `(I, N, S)`. For convenience,
/// we require that both `I` and `N` implement `ToString` so that we can easily log or work with them
/// as owned `String`s.
pub async fn run_component_wrpc_exports<ServeFut, ServeFn, Handler, I, N, S, Fut>(
    addr: &str,
    handler: Handler,
    serve_fn: ServeFn,
) -> anyhow::Result<()>
where
    // The handler must be clonable and safe to send.
    Handler: Clone + Send + 'static,
    // `serve_fn` takes an Arc<MyServer> and a handler, returning a future yielding a vector of
    // (instance, name, invocation stream) tuples.
    ServeFn: Fn(Arc<MyServer>, Handler) -> ServeFut,
    ServeFut: std::future::Future<Output = anyhow::Result<Vec<(I, N, S)>>> + Send,
    // Each invocation stream yields a future that, when awaited, produces an anyhow::Result.
    S: futures::Stream<Item = anyhow::Result<Fut>> + Unpin + Send + 'static,
    // Each individual invocation future resolves to an anyhow::Result.
    Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    // Allow the instance and name types to be converted to owned Strings.
    I: ToString + Clone + Send + 'static,
    N: ToString + Clone + Send + 'static,
{
    // Initialize logging.
    tracing_subscriber::fmt::init();

    // Bind a TCP listener on the specified address.
    let lis = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind TCP listener on '{addr}'"))?;

    // Create the wRPC server instance.
    let srv = Arc::new(wrpc_transport::Server::default());

    // Spawn a task to continuously accept incoming TCP connections.
    let accept_handle = {
        let srv = Arc::clone(&srv);
        tokio::spawn(async move {
            loop {
                if let Err(err) = srv.accept(&lis).await {
                    error!(?err, "failed to accept TCP connection");
                }
            }
        })
    };

    info!("Component is now serving exports on {addr}");

    // Use the provided `serve_fn` to wire up the exports.
    let invocations = serve_fn(Arc::clone(&srv), handler)
        .await
        .context("failed to serve exports")?;

    // Conflate all invocation streams into one unified stream.
    let mut invocations: SelectAll<_> = invocations
        .into_iter()
        .map(|(instance, name, stream)| {
            // Convert the instance and name to owned Strings.
            let instance_str = instance.to_string();
            let name_str = name.to_string();
            info!("serving {} {}", instance_str, name_str);
            stream.map(move |res| (instance_str.clone(), name_str.clone(), res))
        })
        .collect();

    // Prepare a join set to handle spawned tasks.
    let mut tasks = JoinSet::new();
    // Prepare the shutdown signal.
    let shutdown = signal::ctrl_c();
    pin!(shutdown);

    loop {
        select! {
            // Process incoming invocations.
            Some((instance, name, res)) = invocations.next() => {
                match res {
                    Ok(fut) => {
                        debug!(?instance, ?name, "invocation accepted");
                        tasks.spawn(async move {
                            if let Err(err) = fut.await {
                                warn!(?err, "failed to handle invocation");
                            } else {
                                info!(?instance, ?name, "invocation successfully handled");
                            }
                        });
                    },
                    Err(err) => {
                        warn!(?err, ?instance, ?name, "failed to accept invocation");
                    },
                }
            },
            // Process completed tasks.
            Some(task_res) = tasks.join_next() => {
                if let Err(err) = task_res {
                    error!(?err, "failed to join task");
                }
            },
            // Shutdown signal received.
            _ = &mut shutdown => {
                info!("Shutdown signal received. Aborting accept loop and waiting for tasks...");
                accept_handle.abort();
                // Wait for all spawned tasks to complete.
                while let Some(task_res) = tasks.join_next().await {
                    if let Err(err) = task_res {
                        error!(?err, "failed to join task");
                    }
                }
                break;
            }
        }
    }

    Ok(())
}
