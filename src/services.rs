//! Application-owned I/O and bounded CPU capacity.
use anyhow::{Result, bail};
use markview_core::background::{Executor, Task, ThreadExecutor};
use std::{future::Future, pin::Pin, sync::Arc, thread};
use tokio::sync::{Notify, Semaphore, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

type Operation = Pin<Box<dyn Future<Output = ()> + Send>>;
#[derive(Clone)]
pub(crate) struct Handle {
	pub cpu: Arc<dyn Executor>,
	pub transfers: Arc<Semaphore>,
	pub cancel: CancellationToken,
	available: Arc<Notify>,
	_available_wake: markview_core::background::Wake,
	send: mpsc::UnboundedSender<Operation>,
	pub jobs: usize,
}
pub(crate) struct Services {
	pub handle: Handle,
	cpu: Arc<ThreadExecutor>,
	thread: Option<thread::JoinHandle<()>>,
}
impl Services {
	pub fn new(jobs: usize) -> Self {
		// The stack covers the deepest Mermaid recursion the diagram source
		// cap allows; see `images::diagram`.
		let cpu = Arc::new(ThreadExecutor::new(Some(128 * 1024 * 1024)));
		let available = Arc::new(Notify::new());
		let wake = available.clone();
		let available_wake: markview_core::background::Wake =
			Arc::new(move || wake.notify_waiters());
		cpu.on_available(available_wake.clone());
		let cancel = CancellationToken::new();
		let stop = cancel.clone();
		let (send, mut recv) = mpsc::unbounded_channel::<Operation>();
		let thread = thread::Builder::new().name("markview-network".into()).spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().max_blocking_threads(4).enable_all().build().expect("start I/O runtime");
            runtime.block_on(async move {
                let mut running = tokio::task::JoinSet::new();
                loop {
                    tokio::select! {
                        biased;
                        _ = stop.cancelled() => break,
                        Some(operation) = recv.recv() => { running.spawn(operation); },
                        Some(result) = running.join_next(), if !running.is_empty() => {
                            if let Err(error) = result { log::warn!("I/O operation failed: {error}"); }
                        }
                    }
                }
                recv.close();
                while let Ok(operation) = recv.try_recv() { running.spawn(operation); }
                while let Some(result) = running.join_next().await {
                    if let Err(error) = result { log::warn!("I/O operation failed: {error}"); }
                }
            });
        }).expect("start I/O service");
		Self {
			handle: Handle {
				cpu: cpu.clone(),
				transfers: Arc::new(Semaphore::new(jobs.max(1))),
				cancel,
				available,
				_available_wake: available_wake,
				send,
				jobs: jobs.max(1),
			},
			cpu,
			thread: Some(thread),
		}
	}
}
impl Drop for Services {
	fn drop(&mut self) {
		self.handle.cancel.cancel();
		if let Some(thread) = self.thread.take() {
			let _ = thread.join();
		}
		self.cpu.shutdown();
	}
}
impl Handle {
	pub fn submit(
		&self,
		operation: impl Future<Output = ()> + Send + 'static,
	) -> bool {
		!self.cancel.is_cancelled()
			&& self.send.send(Box::pin(operation)).is_ok()
	}
	pub async fn permit(
		&self,
		cancel: &CancellationToken,
	) -> Result<tokio::sync::OwnedSemaphorePermit> {
		tokio::select! {
			biased;
			_ = cancel.cancelled() => bail!("Cancelled"),
			_ = self.cancel.cancelled() => bail!("Cancelled"),
			permit = self.transfers.clone().acquire_owned() => Ok(permit?),
		}
	}
	/// The returned future retains a rejected task instead of copying its input.
	pub async fn compute<T: Send + 'static>(
		&self,
		bytes: usize,
		cancel: &CancellationToken,
		compute: impl FnOnce() -> Result<T> + Send + 'static,
	) -> Result<T> {
		let (send, recv) = oneshot::channel();
		let cancelled = cancel.clone();
		let mut task = Task::new(bytes, move || {
			let result = if cancelled.is_cancelled() {
				Err(anyhow::anyhow!("Cancelled"))
			} else {
				std::panic::catch_unwind(std::panic::AssertUnwindSafe(compute))
					.unwrap_or_else(|_| {
						Err(anyhow::anyhow!("CPU computation failed"))
					})
			};
			let _ = send.send(result);
		});
		loop {
			let ready = self.available.notified();
			tokio::pin!(ready);
			ready.as_mut().enable();
			if cancel.is_cancelled() || self.cancel.is_cancelled() {
				bail!("Cancelled");
			}
			match self.cpu.try_submit(task) {
				Ok(()) => break,
				Err(returned) => task = returned,
			}
			tokio::select! {
				_ = ready => {},
				_ = cancel.cancelled() => bail!("Cancelled"),
				_ = self.cancel.cancelled() => bail!("Cancelled"),
			}
		}
		// Running installation transactions finish before shutdown proceeds.
		recv.await
			.map_err(|_| anyhow::anyhow!("CPU service closed"))?
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn shutdown_delivers_terminal_events_for_accepted_operations() {
		let services = Services::new(1);
		let entered = Arc::new(std::sync::Barrier::new(2));
		let release = Arc::new(std::sync::Barrier::new(2));
		let first = entered.clone();
		let gate = release.clone();
		services.handle.submit(async move {
			first.wait();
			gate.wait();
		});
		entered.wait();
		let handle = services.handle.clone();
		let (done, recv) = std::sync::mpsc::channel();
		assert!(services.handle.submit(async move {
			let token = handle.cancel.child_token();
			assert!(handle.permit(&token).await.is_err());
			done.send(()).unwrap();
		}));
		services.handle.cancel.cancel();
		release.wait();
		drop(services);
		recv.recv_timeout(std::time::Duration::from_secs(5))
			.unwrap();
	}

	#[test]
	fn waiting_transfers_cancel_without_starting_and_shutdown_collects_work() {
		let services = Services::new(1);
		let handle = services.handle.clone();
		let (done, recv) = std::sync::mpsc::channel();
		services.handle.submit(async move {
			let token = handle.cancel.child_token();
			let held = handle.permit(&token).await.unwrap();
			let waiting = token.child_token();
			let permit = handle.permit(&waiting);
			tokio::pin!(permit);
			assert!(futures_util::poll!(permit.as_mut()).is_pending());
			waiting.cancel();
			assert!(permit.await.is_err());
			drop(held);
			done.send(()).unwrap();
		});
		recv.recv_timeout(std::time::Duration::from_secs(5))
			.unwrap();
		drop(services);
	}
}
