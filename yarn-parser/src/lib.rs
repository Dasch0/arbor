#![doc = include_str!("../README.md")]
pub mod pool;

/// scan through a loaded .yarn file, finding variables and instantiating them (with initial values) into a hashmap

pub fn find_variables(_var: &mut usize) {
}

#[cfg(test)]
mod tests {
    use crate::pool;
    fn printer(var: &mut usize, name: &'static str) {
        for i in 0..10506 {
            *var += i;
            println!("thread {} iteration {}", name, var);
        }
    }
    //#[test]
    /// Test a finite threadpool with three printing threads, each modifying different mutable
    /// variables
    fn finite_print() {
        let mut var0 = 0;
        let mut var1 = 1;
        let mut var2 = 2;

        pool::Finite::new()
            .with_job(|| printer(&mut var0, "A"))
            .with_job(|| printer(&mut var1, "B"))
            .with_job(|| printer(&mut var2, "C"))
            .execute()
    }

    //#[test]
    /// Test a scoped threadpool with three printing threads, each modifying different mutable
    /// variables
    fn scope_print() {
        let mut var0 = 0;
        let mut var1 = 1;
        let mut var2 = 2;

        pool::scope(|scope| {
            pool::spawn(scope, || printer(&mut var0, "A"));
            pool::spawn(scope, || printer(&mut var1, "B"));
            pool::spawn(scope, || printer(&mut var2, "C"));
        }).unwrap();
    }


    fn fibonacci(n: u64) -> u64 {
        match n {
            0 => 1,
            1 => 1,
            n => fibonacci(n - 1) + fibonacci(n - 2),
        }
    }

    #[test]
    /// spin up compute jobs to stress the job queue
    fn scope_stress() {
        let mut var0 = 0;
        let res = pool::scope_with_thread_count(6, |scope| {
            for i in 28..32 {
                var0 = i;
                pool::spawn(scope, || {fibonacci(var0);});
                println!("started fib job {}", var0);
            }

            var0 = 1;
            for i in 29..32 {
                var0 = i;
                pool::spawn(scope, || {fibonacci(var0);});
                println!("started fib job {}", var0);
            }
        });

        println!("completed all fib jobs {:?}", res);
    }
}
