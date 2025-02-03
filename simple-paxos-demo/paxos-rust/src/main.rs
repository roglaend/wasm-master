#[cfg(feature = "distributed")]
mod main_distributed;

#[cfg(all(not(feature = "distributed"), feature = "advanced"))]
mod main_advanced;

#[cfg(all(not(feature = "distributed"), not(feature = "advanced")))]
mod main_basic;

fn main() {
    // Initialize logger (for example, to see info and warning messages)
    env_logger::init();

    // Dispatch to the appropriate run() function.
    #[cfg(feature = "distributed")]
    {
        let _ = main_distributed::run();
    }

    #[cfg(all(not(feature = "distributed"), feature = "advanced"))]
    {
        main_advanced::run();
    }

    #[cfg(all(not(feature = "distributed"), not(feature = "advanced")))]
    {
        main_basic::run();
    }
}
