use futures::task::AtomicWaker;
use std::marker::PhantomData;
use std::ops::DerefMut;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;

type UnsendTask = Box<dyn FnOnce(&mut v8::HandleScope) + 'static>;
type SendTask = Box<dyn FnOnce(&mut v8::HandleScope) + Send + 'static>;

static_assertions::assert_not_impl_any!(V8TaskSpawnerFactory: Send);
static_assertions::assert_not_impl_any!(V8TaskSpawner: Send);
static_assertions::assert_impl_all!(V8CrossThreadTaskSpawner: Send);

#[derive(Default)]
pub(crate) struct V8TaskSpawnerFactory {
  // TODO(mmastrac): ideally we wouldn't box if we could use arena allocation and a max submission size
  // TODO(mmastrac): we may want to split the Send and !Send tasks
  /// The set of tasks, non-empty if `has_tasks` is set.
  tasks: Mutex<Vec<SendTask>>,
  /// A flag we can poll without any locks.
  has_tasks: AtomicBool,
  /// The polled waker, woken on task submission.
  waker: AtomicWaker,
  /// This cannot be Send because it may contain `!Send` tasks submitted by a [`V8TaskSpawner`]. It is
  /// only safe to send this object to another thread if you plan on submitting [`Send`] tasks to it,
  /// which is what [`V8CrossThreadTaskSpawner`] does.
  _unsend_marker: PhantomData<*const ()>,
}

impl V8TaskSpawnerFactory {
  pub fn new_same_thread_spawner(self: Arc<Self>) -> V8TaskSpawner {
    V8TaskSpawner {
      tasks: self,
      _unsend_marker: PhantomData,
    }
  }

  pub fn new_cross_thread_spawner(self: Arc<Self>) -> V8CrossThreadTaskSpawner {
    V8CrossThreadTaskSpawner { tasks: self }
  }

  pub fn has_pending_tasks(&self) -> bool {
    self.has_tasks.load(std::sync::atomic::Ordering::SeqCst)
  }

  /// Poll this set of tasks, returning a non-empty set of tasks if there have
  /// been any queued, or registering the waker if not.
  pub fn poll_inner(&self, cx: &mut Context) -> Poll<Vec<UnsendTask>> {
    // Check the flag first -- if it's false we definitely have no tasks
    if !self
      .has_tasks
      .swap(false, std::sync::atomic::Ordering::SeqCst)
    {
      self.waker.register(cx.waker());
      return Poll::Pending;
    }

    let mut lock = self.tasks.lock().unwrap();
    let tasks = std::mem::take(lock.deref_mut());
    if tasks.is_empty() {
      // Unlikely race lost -- the task submission to the queue and flag are not atomic, so it's
      // possible we ended up with an extra poll here. This only shows up under Miri, but as it is
      // possible we do need to handle it.
      self.waker.register(cx.waker());
      return Poll::Pending;
    }

    // SAFETY: we are removing the Send trait as we return the tasks here to prevent
    // these tasks from accidentally leaking to another thread.
    let tasks = unsafe { std::mem::transmute(tasks) };
    Poll::Ready(tasks)
  }

  fn spawn(&self, task: SendTask) {
    self.tasks.lock().unwrap().push(task);
    // TODO(mmastrac): can we use a looser ordering here?
    self
      .has_tasks
      .store(true, std::sync::atomic::Ordering::SeqCst);
    self.waker.wake();
  }
}

/// Allows for submission of v8 tasks on the same thread.
#[derive(Clone)]
pub struct V8TaskSpawner {
  // TODO(mmastrac): can we split the waker into a send and !send one?
  tasks: Arc<V8TaskSpawnerFactory>,
  _unsend_marker: PhantomData<*const ()>,
}

impl V8TaskSpawner {
  /// Spawn a task that runs within the [`crate::JsRuntime`] event loop from the same thread
  /// that the runtime is running on. This function is re-entrant-safe and may be called from
  /// ops, from outside of a [`v8::HandleScope`] in a plain `async`` task, or even from within
  /// another, previously-spawned task.
  ///
  /// The task is handed off to be run the next time the event loop is polled, and there are
  /// no guarantees as to when this may happen.
  ///
  /// # Important Notes
  ///
  /// The task shares the same [`v8::HandleScope`] as the core event loop, which means that it
  /// must maintain the scope in a valid state to avoid corrupting or destroying the runtime.
  ///
  /// For example, if the code called by this task can raise an exception, the task must ensure
  /// that it calls that code within a new [`v8::TryCatch`] to avoid the exception leaking to the
  /// event loop's [`v8::HandleScope`].
  pub fn spawn<F>(&self, f: F)
  where
    F: FnOnce(&mut v8::HandleScope) + 'static,
  {
    // SAFETY: we are transmuting Send into a !Send handle but we can guarantee this object will never
    // leave the current thread because `V8TaskSpawner` is !Send.
    let task: Box<dyn FnOnce(&mut v8::HandleScope<'_>)> = Box::new(f);
    let task: Box<dyn FnOnce(&mut v8::HandleScope<'_>) + Send> =
      unsafe { std::mem::transmute(task) };
    self.tasks.spawn(task)
  }
}

/// Allows for submission of v8 tasks on any thread.
#[derive(Clone)]
pub struct V8CrossThreadTaskSpawner {
  tasks: Arc<V8TaskSpawnerFactory>,
}

// SAFETY: the underlying V8TaskSpawnerFactory is not Send, but we always submit Send tasks
// to it from this spawner.
unsafe impl Send for V8CrossThreadTaskSpawner {}

impl V8CrossThreadTaskSpawner {
  /// Spawn a task that runs within the [`crate::JsRuntime`] event loop, potentially (but not
  /// required to be) from a different thread than the runtime is running on.
  ///
  /// The task is handed off to be run the next time the event loop is polled, and there are
  /// no guarantees as to when this may happen.
  ///
  /// # Important Notes
  ///
  /// The task shares the same [`v8::HandleScope`] as the core event loop, which means that it
  /// must maintain the scope in a valid state to avoid corrupting or destroying the runtime.
  ///
  /// For example, if the code called by this task can raise an exception, the task must ensure
  /// that it calls that code within a new [`v8::TryCatch`] to avoid the exception leaking to the
  /// event loop's [`v8::HandleScope`].
  pub fn spawn<F>(&self, f: F)
  where
    F: FnOnce(&mut v8::HandleScope) + Send + 'static,
  {
    self.tasks.spawn(Box::new(f))
  }

  /// Spawn a task that runs within the [`crate::JsRuntime`] event loop from a different thread
  /// than the runtime is running on.
  ///
  /// This function will deadlock if called from the same thread as the [`crate::JsRuntime`], and
  /// there are no checks for this case.
  ///
  /// As this function blocks until the task has run to completion (or panics/deadlocks), it is
  /// safe to borrow data from the local environment and use it within the closure.
  ///
  /// The task is handed off to be run the next time the event loop is polled, and there are
  /// no guarantees as to when this may happen, however the function will not return until the
  /// task has been fully run to completion.
  ///
  /// # Important Notes
  ///
  /// The task shares the same [`v8::HandleScope`] as the core event loop, which means that it
  /// must maintain the scope in a valid state to avoid corrupting or destroying the runtime.
  ///
  /// For example, if the code called by this task can raise an exception, the task must ensure
  /// that it calls that code within a new [`v8::TryCatch`] to avoid the exception leaking to the
  /// event loop's [`v8::HandleScope`].
  pub fn spawn_blocking<'a, F, T>(&self, f: F) -> T
  where
    F: FnOnce(&mut v8::HandleScope) -> T + Send + 'a,
    T: Send + 'a,
  {
    let (tx, rx) = std::sync::mpsc::sync_channel(0);
    let task: Box<dyn FnOnce(&mut v8::HandleScope<'_>) + Send> =
      Box::new(|scope| {
        let r = f(scope);
        _ = tx.send(r);
      });
    // SAFETY: We can safely transmute to the 'static lifetime because we guarantee this method will either
    // complete fully by the time it returns, deadlock or panic.
    let task: SendTask = unsafe { std::mem::transmute(task) };
    self.tasks.spawn(task);
    rx.recv().unwrap()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::future::poll_fn;
  use tokio::task::LocalSet;

  #[test]
  fn test_spawner_serial() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
      .worker_threads(1)
      .build()
      .unwrap();
    runtime.block_on(async {
      let factory = Arc::<V8TaskSpawnerFactory>::default();
      let cross_thread_spawner = factory.clone().new_cross_thread_spawner();
      let local_set = LocalSet::new();

      const COUNT: usize = 100;

      let task = runtime.spawn(async move {
        for _ in 0..COUNT {
          cross_thread_spawner.spawn(|_| {});
        }
      });

      local_set.spawn_local(async move {
        let mut count = 0;
        loop {
          count += poll_fn(|cx| factory.poll_inner(cx)).await.len();
          if count >= COUNT {
            break;
          }
        }
      });

      local_set.await;
      _ = task.await;
    });
  }

  #[test]
  fn test_spawner_parallel() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
      .worker_threads(1)
      .build()
      .unwrap();
    runtime.block_on(async {
      let factory = Arc::<V8TaskSpawnerFactory>::default();
      let cross_thread_spawner = factory.clone().new_cross_thread_spawner();
      let local_set = LocalSet::new();

      const COUNT: usize = 100;
      let mut tasks = vec![];
      for _ in 0..COUNT {
        let cross_thread_spawner = cross_thread_spawner.clone();
        tasks.push(runtime.spawn(async move {
          cross_thread_spawner.spawn(|_| {});
        }));
      }

      local_set.spawn_local(async move {
        let mut count = 0;
        loop {
          count += poll_fn(|cx| factory.poll_inner(cx)).await.len();
          if count >= COUNT {
            break;
          }
        }
      });

      local_set.await;
      for task in tasks.drain(..) {
        _ = task.await;
      }
    });
  }
}
