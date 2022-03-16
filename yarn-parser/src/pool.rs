/// A small & opinionated threadpool implementation with minimal runtime costs. Most of this crate is
/// adapted from bits and pieces of rayon: https://github.com/rayon-rs/rayon/tree/aa063b1c00cfa1fa74b8193f657998fba46c45b3
use bumpalo::{boxed::Box, Bump};
use std::{
    io,
    sync::atomic::{AtomicBool, Ordering},
    thread,
};

/// Maximum supported threads
pub const MAX_THREADS: usize = 7;

/// Data for simple parallel execution of a finite number of jobs. Each job gets its own thread.
/// Add jobs by passing in closures using [with_job], then run all tasks using [execute]. All jobs
/// are guaranteed to complete and join when execute returns, and all threads will be destroyed at
/// that time.
///
/// This should be used for extremely simple cases, when you have a small set (<core count) number
/// of long running (> 1ms) separate tasks that you only need to run once. If you need a varying
/// or large number of jobs to run, use [pool::Scope] instead.
// TODO: Replace vec with inline storage
pub struct Finite {
    arena: Bump,
    jobs: Vec<job::Ref>,
    threads: Vec<thread::JoinHandle<()>>,
}

impl Finite {
    pub fn new() -> Self {
        Self {
            arena: Bump::new(),
            jobs: Vec::with_capacity(MAX_THREADS),
            threads: Vec::with_capacity(MAX_THREADS),
        }
    }
    pub fn execute(mut self) {
        for job_ref in self.jobs {
            // Implementation note:
            //  This is unsafe because we are eliding the lifetime requirements of the job, which rather
            //  than being static is located on a temporarily allocated arena. Additionally, any
            //  captured context in the closures to execute is no longer borrow checked.
            //
            //  However, the sequence of code below, where all threads are immediately joined, followed
            //  by the deallocation of the arena and all data as it is dropped, is guaranteed to
            //  complete before this function returns and is safe for external users
            self.threads
                .push(thread::spawn(move || unsafe { job_ref.execute() }));
        }

        // FIXME: this currently wastes a thread spin-locking on the other thread joins. Probably
        // should handle this via signals
        for thread in self.threads {
            thread.join().unwrap(); // continue a delayed panic
        }
    }

    /// Add a job to be executed in parallel by calling [Scope::execute]
    pub fn with_job<F>(mut self, f: F) -> Self
    where
        F: FnOnce() + Send,
    {
        // the job is allocated onto the scope's memory arena here. This ensures that the job will
        // live long enough to execute the threadpool (see impl note below)
        //
        // FIXME: this is probably not entirely necessary, but for now this code closely follows the
        // rayon impl of HeapJob, just allocated onto the arena instead
        let job = bumpalo::boxed::Box::new_in(job::Inline::new(f), &self.arena);
        // Implementation note:
        //  This is unsafe because we are eliding the lifetime requirements of the job, which rather
        //  than being static is located on a temporarily allocated arena. Additionally, any
        //  captured context in the closures to execute is no longer borrow checked.
        //
        //  However, the sequence of code below, where all threads are immediately joined, followed
        //  by the deallocation of the arena and all data as it is dropped, is guaranteed to
        //  complete before this function returns and is safe for external users
        unsafe {
            self.jobs.push(job::Ref::from_arena_job(job));
        }
        self
    }
}

pub struct Scope<const Q: usize> {
    arena: *const Bump,
    queue: *const ach_ring::Ring<job::Ref, Q>,
    threads: *const [thread::JoinHandle<()>],
}

/// public interface to create and execute a scoped threadpool, creating a thread-per-core. Returns
/// an error if the current processor parallelism cannot be queried. It is recommended to fall back
/// to [scope_with_threads] to manually define a fallback thread count for such cases

pub fn scope<F, R>(f: F) -> io::Result<R>
where
    F: FnOnce(&mut Scope<MAX_THREADS>) -> R,
{
    Ok(scope_with_thread_count::<F, R>(
        thread::available_parallelism()?.get(),
        f,
    ))
}

/// public interface to create and execute a scoped threadpool
pub fn scope_with_thread_count<F, R>(thread_count: usize, f: F) -> R
where
    F: FnOnce(&mut Scope<MAX_THREADS>) -> R,
{
    unsafe {
        let arena = Bump::new();
        // we need to tie the job queue & terminate flag to the scope lifetime, and then elide the lifetime to ensure
        // that we can pass the receiver to our threads. This method guarantees that all threads are
        // ended before the value is destroyed
        let queue = Box::into_raw(Box::new_in(ach_ring::Ring::<job::Ref, MAX_THREADS>::new(), &arena));
        let terminate = Box::into_raw(Box::new_in(AtomicBool::new(false), &arena));
        let mut threads = Vec::with_capacity(thread_count);

        // this is where the lifetime is elided, from here, this fucntion *must* guarantee that all
        // threads have joined before returning, otherwise the arena will be deallocated
        let q_ref = queue.as_ref().unwrap();
        let t_ref = terminate.as_ref().unwrap();
        for _ in 0..thread_count {
            threads.push(thread::spawn(|| {
                worker(
                    &thread::current(),
                    q_ref,
                    t_ref,
                );
            }));
        }

        let mut scope = Scope {
            arena: &arena,
            queue: queue.clone(),
            threads: threads.as_slice(),
        };

        let ret = f(&mut scope);

        // park until queue is empty, worker threads will wake owner thread up
        while !(*queue).is_empty() {
            thread::park()
        }
        // now that the queue is empty, terminate all threads and wait for join
        (*terminate).store(true, Ordering::Release);
        for thread in threads {
            thread.thread().unpark();
            thread.join().unwrap(); // continue a delayed panic
        }
        ret
    }
}

/// spawn a new task to start executing in parallel. Call this within a scoped closure
pub fn spawn<F>(scope: &mut Scope<MAX_THREADS>, f: F)
where
    F: FnOnce() + Send,
{
    // the job is allocated onto the scope's memory arena here. This ensures that the job will
    // live long enough to execute the threadpool (see impl note below)
    //
    // FIXME: this is probably not entirely necessary, but for now this code closely follows the
    // rayon impl of HeapJob, just allocated onto the arena instead
    // Implementation note:
    //  This is unsafe because we are eliding the lifetime requirements of the job, which rather
    //  than being static is located on a temporarily allocated arena. Additionally, any
    //  captured context in the closures to execute is no longer borrow checked.
    //
    //  However, the sequence of code below, where all threads are immediately joined, followed
    //  by the deallocation of the arena and all data as it is dropped, is guaranteed to
    //  complete before this function returns and is safe for external users
    unsafe {
        let job = bumpalo::boxed::Box::new_in(job::Inline::new(f), scope.arena.as_ref().unwrap());
        let job_ref = job::Ref::from_arena_job(job);
        while let Err(_) = (*scope.queue).push(job_ref.clone()) {};

        // notify all workers that we just added a new job
        // FIXME: inefficient to call unpark n times for some thread, probably causes contention
        // too
        for handle in &(*scope.threads) {
            handle.thread().unpark()
        }

    }
}

/// helper function defining the execution loop for worker threads
///
/// The user must guarantee job refs passed in must stay valid until a job completes, and thus
/// worker loops are unsafe
unsafe fn worker<'q, const Q: usize>(
    owner_thread: &thread::Thread,
    incoming_jobs: &ach_ring::Ring<job::Ref, Q>,
    terminate: &AtomicBool,
) {
    // TODO: abort failsafe

    // loop until terminate is set true, this must happen exactly once, so relaxed ordering is fine
    while !terminate.load(Ordering::Relaxed) {
        match incoming_jobs.pop() {
            Ok(job) => job.execute(),        // execute the newly available job
            Err(_) => {
                owner_thread.unpark(); // ring empty, signal to the owner thread that we have no work to do
                thread::park(); // wait for owner thread to notify us by unparking
            }
        }
    }
}

/// This submodule implements the data structures required to run parallel jobs in scoped
/// threadpools, with the general goals of providing the closure and job data to a worker thread,
/// and allowing the threadpool scope to manually manage memory
///
/// This implementation is mostly derived from [rayon]'s HeapJob, with the main difference being
/// [Pool] scopes use [bumpalo] for temporary memory arenas
mod job {
    use std::{cell::UnsafeCell, mem};
    /// Trait defining jobs to run in parallel
    pub trait Job {
        /// Unsafe: this may be called from a different thread than the one
        /// which scheduled the job, so the implementer must ensure the
        /// appropriate traits are met, whether `Send`, `Sync`, or both.
        unsafe fn execute(this: *const Self);
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
        unsafe fn new<JOB>(data: *const JOB) -> Ref
        where
            JOB: Job,
        {
            let fn_ptr: unsafe fn(*const JOB) = <JOB as Job>::execute;
            Self {
                // this type erasure lets us define our own lifetime requirements for the underlying job`
                pointer: data as *const (),
                execute_fn: mem::transmute(fn_ptr),
            }
        }

        pub unsafe fn execute(&self) {
            (self.execute_fn)(self.pointer)
        }

        /// Create a job reference from a job allocated on a memory arena (implemented with [bumpalo]).
        /// Note that this method is highly unsafe, it is up to the user to ensure the memory arena
        /// lives until the job is completed
        pub unsafe fn from_arena_job<J>(job: bumpalo::boxed::Box<J>) -> Ref
        where
            J: Job,
        {
            let this: *const J = mem::transmute(job);
            Ref::new(this)
        }
    }

    /// A job where the job data and closure is stored inline. This type of job generally needs to be allocated
    /// somewhere in order to work with threadpools.
    pub struct Inline<F>
    where
        F: FnOnce() + Send,
    {
        /// Pointer to the storage on the arena with interior Mutability. The Option will allow us to ensure that the job runs
        /// once-and-only-once
        job: UnsafeCell<Option<F>>,
    }

    impl<F> Inline<F>
    where
        F: FnOnce() + Send,
    {
        pub fn new(func: F) -> Self {
            Self {
                job: UnsafeCell::new(Some(func)),
            }
        }
    }

    impl<F> Job for Inline<F>
    where
        F: FnOnce() + Send,
    {
        unsafe fn execute(this: *const Self) {
            // Here, we 'take' the pointer out of the Option, leaving None
            let job = (*(*this).job.get()).take().unwrap();
            (job)()
        }
    }
}
