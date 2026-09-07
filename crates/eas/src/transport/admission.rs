use crate::{EasError, RequestSafety, Result};
use std::time::Duration;
use tokio::sync::{Semaphore, SemaphorePermit};

pub(super) static HTTP_GATE: Gate = Gate::new(8, 2, 32);

pub(super) struct Gate {
    reads: Semaphore,
    writes: Semaphore,
    waiting: Semaphore,
}
impl Gate {
    const fn new(reads: usize, writes: usize, waiting: usize) -> Self {
        Self {
            reads: Semaphore::const_new(reads),
            writes: Semaphore::const_new(writes),
            waiting: Semaphore::const_new(waiting),
        }
    }

    pub(super) async fn acquire(&self, safety: RequestSafety) -> Result<SemaphorePermit<'_>> {
        let active = match safety {
            RequestSafety::RetrySafe => &self.reads,
            RequestSafety::Mutation => &self.writes,
        };
        if let Ok(permit) = active.try_acquire() {
            return Ok(permit);
        }
        let waiting = self.waiting.try_acquire().map_err(|_| EasError::ResourceBusy)?;
        let permit = tokio::time::timeout(Duration::from_secs(30), active.acquire())
            .await
            .map_err(|_| EasError::ResourceBusy)?
            .map_err(|_| EasError::ResourceBusy)?;
        drop(waiting);
        Ok(permit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn queue_is_bounded_and_cancelled_waiter_never_acquires_later() -> anyhow::Result<()> {
        let gate = Gate::new(1, 1, 1);
        let active = gate.acquire(RequestSafety::Mutation).await?;
        let mut waiting = Box::pin(gate.acquire(RequestSafety::Mutation));
        tokio::select! {
            biased;
            result = &mut waiting => anyhow::bail!("unexpected early admission: {:?}", result.err()),
            () = tokio::task::yield_now() => {}
        }
        assert!(matches!(gate.acquire(RequestSafety::Mutation).await, Err(EasError::ResourceBusy)));
        drop(waiting);
        drop(active);
        // Cancellation is represented by dropping the enclosing future, as in the MCP handler.
        let gate = Gate::new(1, 1, 1);
        let active = gate.acquire(RequestSafety::Mutation).await?;
        {
            let mut waiter = Box::pin(gate.acquire(RequestSafety::Mutation));
            tokio::select! {
                biased;
                result = &mut waiter => anyhow::bail!("unexpected early admission: {:?}", result.err()),
                () = tokio::task::yield_now() => {}
            }
        }
        drop(active);
        let permit = gate.acquire(RequestSafety::Mutation).await?;
        assert_eq!(gate.waiting.available_permits(), 1);
        drop(permit);
        Ok(())
    }
}
