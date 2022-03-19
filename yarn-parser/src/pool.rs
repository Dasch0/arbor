use staticvec::StaticVec;
/// Simple & opinionated parallelism tools with minimal dependencies or runtime overhead. Most of
/// this crate is adapted from bits and pieces of rayon:
/// https://github.com/rayon-rs/rayon/tree/aa063b1c00cfa1fa74b8193f657998fba46c45b3
use std::{marker::PhantomData, thread};

pub use job::InlineJob;

#[inline]
/// Create a job, able to be executed in parallel, from a provided closure. The closure must
/// implement `FnMut`
pub fn job<F>(f: F) -> job::InlineJob<F>
where
    F: FnMut() + Send,
{
    job::InlineJob::new(f)
}

/// Create a scope for jobs to run in parallel. All jobs are guaranteed to complete when the
/// provided closure returns.
///
/// NOTE: this is intended for jobs with a finite run time. If a job entires an infinite loop
/// the scope will never return
#[inline]
pub fn scope<'scope, F, const NUM_THREADS: usize>(f: F)
where
    F: FnOnce(Scope<'scope, NUM_THREADS>) -> Scope<'scope, NUM_THREADS>,
{
    let scope = Scope {
        threads: StaticVec::new(),
        marker: PhantomData::default(),
    };

    let scope = (f)(scope);

    for t in scope.threads {
        t.join().unwrap();
    }
}

/// Stores data needed to run parallel jobs within a scope. The generic parameter indicates the
/// number of threads (and thus jobs) that can be created.
///
/// Spawn a new job to be run in parallel with [Scope::spawn]
pub struct Scope<'scope, const NUM_THREADS: usize> {
    threads: StaticVec<thread::JoinHandle<()>, NUM_THREADS>,
    marker: PhantomData<::std::cell::Cell<&'scope mut ()>>,
}

impl<'scope, const N: usize> Scope<'scope, N> {
    /// Stage a job to be executed in parallel. Guaranteed to complete before
    /// the returned handle is dropped.
    ///
    /// # Panic
    /// If the maximum number of threads defined by the scope have already been used
    #[inline]
    pub fn spawn<J>(&mut self, j: &'scope J)
    where
        J: job::Work + Send,
    {
        unsafe {
            let job_ref = j.as_job_ref();
            // try to push, if the vector is full, throw a compile error. This works because
            // staticvec has const implementations for checking if the threads are full
            self.check_thread_available();
            self.threads.push(thread::spawn(move || job_ref.execute()));
        }
    }

    /// const helper to compile time panic if the scope's threads are all in use
    const fn check_thread_available(&self) {
        if self.threads.is_full() {
            panic!("Scope attempted to spawn more jobs than threads are available. 
                   Recommended to define the scope with more threads");
        }
    }
}

/// This submodule implements the data structures required to run parallel jobs in scoped
/// threadpools, with the general goals of providing the closure and job data to a worker thread,
/// and allowing the threadpool scope to manually manage memory and not force everything to be
/// 'static
///
/// Everything in this module is highly unsafe, it's outer users must guarantee safety
///
/// This implementation is mostly derived from rayon's `HeapJob`
mod job {
    use std::{cell::UnsafeCell, mem};
    /// Trait defining work to be run in parallel
    pub trait Work {
        unsafe fn execute(this: *const Self);
        unsafe fn as_job_ref(&self) -> Ref;
    }

    /// An unsafe job reference for which the caller guarantees the availability of a
    /// job's data, rather than the inner job's lifetime
    #[derive(Clone)]
    pub struct Ref {
        pointer: *const (),
        execute_fn: unsafe fn(*const ()),
    }

    unsafe impl Send for Ref {}
    unsafe impl Sync for Ref {}

    impl Ref {
        /// Caller must assert that the job's data will remain valid until the job is executed
        pub unsafe fn new<J>(data: *const J) -> Ref
        where
            J: Work,
        {
            let fn_ptr: unsafe fn(*const J) = <J as Work>::execute;
            Self {
                // this type erasure lets us define our own lifetime requirements for the underlying job`
                pointer: data as *const (),
                execute_fn: mem::transmute(fn_ptr),
            }
        }

        pub unsafe fn execute(&self) {
            (self.execute_fn)(self.pointer)
        }
    }

    pub struct InlineJob<F> {
        job: UnsafeCell<F>,
    }

    impl<'scope, F> InlineJob<F>
    where
        F: FnMut() + Send + 'scope,
    {
        pub fn new(func: F) -> Self {
            Self {
                job: UnsafeCell::new(func),
            }
        }
    }

    impl<'scope, F> Work for InlineJob<F>
    where
        F: FnMut() + Send + 'scope,
    {
        unsafe fn execute(this: *const Self) {
            let job = (*this).job.get();
            (*job)()
        }

        unsafe fn as_job_ref(&self) -> Ref {
            Ref::new(self)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::pool;
    fn printer(var: &mut usize, name: &'static str) {
        for i in 0..10 {
            *var += i;
            println!("thread {} iteration {}", name, var);
        }
    }
    #[test]
    /// Test a scoped threadpool with three printing threads, each modifying different mutable
    /// variables
    fn scope_print() {
        let mut var0 = 0;
        let mut var1 = 1;
        let mut var2 = 2;

        let job0 = pool::job(|| printer(&mut var0, "A"));
        let job1 = pool::job(|| printer(&mut var1, "A"));
        let job2 = pool::job(|| printer(&mut var2, "A"));

        pool::scope::<_, 3>(|mut scope| {
            scope.spawn(&job0);
            scope.spawn(&job1);
            scope.spawn(&job2);
            scope
        });
    }

    fn fibonacci(n: u64) -> u64 {
        match n {
            0 => 1,
            1 => 1,
            n => fibonacci(n - 1) + fibonacci(n - 2),
        }
    }

    #[test]
    fn scope_stress() {
        let mut var0 = 0;
        let mut var1 = 0;
        let mut var2 = 0;
        let mut var3 = 0;
        let mut var4 = 0;
        let mut var5 = 0;
        let mut var6 = 0;
        let mut var7 = 0;

        let mut job0 = pool::job(|| {
            var0 = fibonacci(20);
        });
        let mut job1 = pool::job(|| {
            var1 = fibonacci(20);
        });
        let mut job2 = pool::job(|| {
            var2 = fibonacci(20);
        });

        let mut job3 = pool::job(|| {
            var3 = fibonacci(20);
        });

        let mut job4 = pool::job(|| {
            var4 = fibonacci(20);
        });

        let mut job5 = pool::job(|| {
            var5 = fibonacci(20);
        });

        let mut job6 = pool::job(|| {
            var6 = fibonacci(20);
        });

        let mut job7 = pool::job(|| {
            var7 = fibonacci(20);
        });

        pool::scope(|mut scope: pool::Scope<8>| {
            scope.spawn(&mut job0);
            scope.spawn(&mut job1);
            scope.spawn(&mut job2);
            scope.spawn(&mut job3);
            scope.spawn(&mut job4);
            scope.spawn(&mut job5);
            scope.spawn(&mut job6);
            scope.spawn(&mut job7);
            scope
        });
    }
}
