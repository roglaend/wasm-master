#[cfg(feature = "advanced")]
mod main_advanced;

#[cfg(not(feature = "advanced"))]
mod main_basic;

fn main() {
    #[cfg(feature = "advanced")]
    main_advanced::run();

    #[cfg(not(feature = "advanced"))]
    main_basic::run();
}
